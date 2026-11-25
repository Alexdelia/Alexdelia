use std::collections::BTreeMap;

use super::clock::Clock;
use super::feeder::Flight;
use super::ranking::Cell;
use super::schedule::{Shot, Throw};

pub(super) struct Feed {
	pub(super) lane: usize,
	pub(super) frame: usize,
	pub(super) fed_before: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Shown {
	pub(super) on: f64,
	pub(super) off: Option<f64>,
}

#[derive(Clone, Copy)]
struct Run {
	lane: usize,
	start: usize,
	end: usize,
	on: f64,
	last_landing_ms: f64,
}

pub(super) fn feed_list(row_list: &[Vec<Cell>]) -> Vec<Feed> {
	let mut list: Vec<Feed> = row_list
		.iter()
		.enumerate()
		.flat_map(|(lane, cells)| lane_feeds(lane, cells))
		.collect();
	list.sort_by_key(|feed| (feed.frame, feed.lane));
	list
}

fn lane_feeds(lane: usize, cells: &[Cell]) -> Vec<Feed> {
	let mut fed = 0;
	let mut list = Vec::new();
	for (frame, cell) in cells.iter().enumerate() {
		if cell.visible && cell.lines > fed {
			list.push(Feed {
				lane,
				frame,
				fed_before: fed,
			});
			fed = cell.lines;
		}
	}
	list
}

pub(super) fn throw_list(
	feed_list: &[Feed],
	tick_ms: u32,
	flight_ms: impl Fn(&Feed) -> i64,
) -> Vec<Throw> {
	feed_list
		.iter()
		.map(|feed| Throw {
			lane: feed.lane,
			deadline_ms: i64::from(tick_ms)
				.saturating_mul(i64::try_from(feed.frame).unwrap_or(i64::MAX)),
			flight_ms: flight_ms(feed),
		})
		.collect()
}

pub(super) fn paired<'a>(
	feed_list: &'a [Feed],
	shot_list: &'a [Shot],
) -> Vec<(&'a Feed, &'a Shot)> {
	let mut by_lane: BTreeMap<usize, Vec<&'a Feed>> = BTreeMap::new();
	for feed in feed_list {
		by_lane.entry(feed.lane).or_default().push(feed);
	}
	let mut cursor: BTreeMap<usize, usize> = BTreeMap::new();
	shot_list
		.iter()
		.filter_map(|shot| {
			let taken = cursor.entry(shot.lane).or_default();
			let feed = by_lane.get(&shot.lane)?.get(*taken).copied();
			*taken += 1;
			feed.map(|feed| (feed, shot))
		})
		.collect()
}

pub(super) fn growth(lane: usize, cells: &[Cell], flight_list: &[Flight]) -> Vec<(u32, u64)> {
	let mut landed: Vec<&Flight> = flight_list
		.iter()
		.filter(|flight| flight.lane == lane)
		.collect();
	landed.sort_by_key(|flight| flight.land_ms);
	let mut fed = 0;
	landed
		.iter()
		.map(|flight| {
			fed = cells
				.get(flight.frame)
				.map_or(fed, |cell| fed.max(cell.lines));
			(flight.land_ms, fed)
		})
		.collect()
}

fn visible_runs(cells: &[Cell]) -> Vec<(usize, usize)> {
	let mut run_list: Vec<(usize, usize)> = Vec::new();
	for (frame, cell) in cells.iter().enumerate() {
		if !cell.visible {
			continue;
		}
		match run_list.last_mut() {
			Some(run) if run.1 + 1 == frame => run.1 = frame,
			_ => run_list.push((frame, frame)),
		}
	}
	run_list
}

fn run_list(cells_list: &[&[Cell]], flight_list: &[Flight], clock: &Clock) -> Vec<Run> {
	cells_list
		.iter()
		.enumerate()
		.flat_map(|(lane, cells)| {
			visible_runs(cells)
				.into_iter()
				.map(|(start, end)| {
					let fed: Vec<&Flight> = flight_list
						.iter()
						.filter(|flight| {
							flight.lane == lane && (start..=end).contains(&flight.frame)
						})
						.collect();
					let shows_ms = if start == 0 {
						0.0
					} else {
						clock.ms(clock.lead + start)
					};
					let on = fed
						.iter()
						.map(|flight| f64::from(flight.fire_ms))
						.fold(shows_ms, f64::min);
					Run {
						lane,
						start,
						end,
						on,
						last_landing_ms: fed
							.iter()
							.map(|flight| f64::from(flight.land_ms))
							.fold(on, f64::max),
					}
				})
				.collect::<Vec<_>>()
		})
		.collect()
}

pub(super) fn shown_list(
	cells_list: &[&[Cell]],
	flight_list: &[Flight],
	clock: &Clock,
) -> Vec<Vec<Shown>> {
	let run_list = run_list(cells_list, flight_list, clock);
	cells_list
		.iter()
		.enumerate()
		.map(|(lane, cells)| {
			run_list
				.iter()
				.filter(|run| run.lane == lane)
				.map(|run| Shown {
					on: run.on,
					off: (run.end + 1 < cells.len()).then(|| hide_ms(run, &run_list, clock)),
				})
				.collect()
		})
		.collect()
}

fn hide_ms(run: &Run, run_list: &[Run], clock: &Clock) -> f64 {
	let boundary = run.end + 1;
	let mut entering: Vec<&Run> = run_list
		.iter()
		.filter(|other| other.start == boundary)
		.collect();
	entering.sort_by(|left, right| {
		left.on
			.total_cmp(&right.on)
			.then(left.lane.cmp(&right.lane))
	});
	let mut leaving: Vec<&Run> = run_list
		.iter()
		.filter(|other| other.end == run.end)
		.collect();
	leaving.sort_by(|left, right| {
		left.last_landing_ms
			.total_cmp(&right.last_landing_ms)
			.then(left.lane.cmp(&right.lane))
	});
	let rank = leaving
		.iter()
		.position(|other| other.lane == run.lane)
		.unwrap_or(0);
	entering.get(rank).map_or_else(
		|| clock.ms(clock.lead + boundary),
		|partner| run.last_landing_ms.max(partner.on),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn cell(lines: u64, visible: bool) -> Cell {
		Cell {
			lines,
			visible,
			..Cell::default()
		}
	}

	fn shot(lane: usize, fire_ms: i64) -> Shot {
		Shot {
			lane,
			enter_ms: fire_ms,
			fire_ms,
			land_ms: fire_ms + 500,
		}
	}

	#[test]
	fn reentry_feeds_lines_grown_while_hidden() {
		let cells = [
			cell(10, true),
			cell(10, true),
			cell(30, false),
			cell(30, true),
			cell(30, true),
			cell(40, true),
		];
		let feeds = lane_feeds(3, &cells);
		let frame_list: Vec<usize> = feeds.iter().map(|feed| feed.frame).collect();
		assert_eq!(frame_list, [0, 3, 5]);
		assert!(feeds.iter().all(|feed| feed.lane == 3));
		let fed_list: Vec<u64> = feeds.iter().map(|feed| feed.fed_before).collect();
		assert_eq!(fed_list, [0, 10, 30]);
	}

	#[test]
	fn shots_pair_with_their_own_lane_feeds() {
		let row_list = vec![
			vec![cell(5, true), cell(9, true)],
			vec![cell(0, false), cell(3, true)],
		];
		let feeds = feed_list(&row_list);
		let throws = throw_list(&feeds, 100, |feed| i64::try_from(feed.lane).unwrap() + 400);
		let deadline_list: Vec<(usize, i64, i64)> = throws
			.iter()
			.map(|throw| (throw.lane, throw.deadline_ms, throw.flight_ms))
			.collect();
		assert_eq!(deadline_list, [(0, 0, 400), (0, 100, 400), (1, 100, 401)]);
		let shot_list = [shot(1, 10), shot(0, 20), shot(0, 30), shot(2, 40)];
		let pair_list: Vec<(usize, usize, i64)> = paired(&feeds, &shot_list)
			.iter()
			.map(|(feed, shot)| (feed.lane, feed.frame, shot.fire_ms))
			.collect();
		assert_eq!(pair_list, [(1, 1, 10), (0, 0, 20), (0, 1, 30)]);
	}

	#[test]
	fn growth_follows_landing_order_and_never_shrinks() {
		let cells = [cell(4, true), cell(9, true), cell(12, true)];
		let flight = |frame: usize, land_ms: u32| Flight {
			lane: 0,
			name: "Rust",
			frame,
			enter_ms: 0,
			fire_ms: 0,
			land_ms,
			to_x: 0.0,
			landing_y: 0.0,
		};
		let flight_list = [
			flight(0, 300),
			flight(2, 500),
			flight(1, 600),
			flight(7, 700),
		];
		assert_eq!(
			growth(0, &cells, &flight_list),
			[(300, 4), (500, 12), (600, 12), (700, 12)]
		);
	}

	#[test]
	fn leaving_without_newcomer_fades_on_its_frame() {
		let clock = Clock::new(10, 900, 0);
		let leaving = Run {
			lane: 0,
			start: 0,
			end: 4,
			on: 0.0,
			last_landing_ms: 700.0,
		};
		let staying = Run {
			lane: 1,
			start: 0,
			end: 9,
			on: 0.0,
			last_landing_ms: 0.0,
		};
		let hide = hide_ms(&leaving, &[leaving, staying], &clock);
		assert!((hide - clock.ms(clock.lead + 5)).abs() < f64::EPSILON);
		let newcomer = Run {
			lane: 2,
			start: 5,
			end: 9,
			on: 650.0,
			last_landing_ms: 900.0,
		};
		let handed = hide_ms(&leaving, &[leaving, newcomer], &clock);
		assert!((handed - 700.0).abs() < f64::EPSILON);
	}
}
