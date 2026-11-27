use std::collections::BTreeMap;

use super::clock::Clock;
use super::feeder::Flight;
use super::format::whole_ms;
use super::ranking::{Cell, cell_at};
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
	let mut previous = 0;
	let mut list = Vec::new();
	for (frame, cell) in cells.iter().enumerate() {
		if cell.visible && cell.lines > previous {
			list.push(Feed {
				lane,
				frame,
				fed_before: previous,
			});
		}
		previous = cell.lines;
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

pub(super) fn bar_steps(
	lane: usize,
	cells: &[Cell],
	flight_list: &[Flight],
	clock: &Clock,
) -> Vec<(u32, u64)> {
	let lines = |frame: usize| cell_at(cells, frame).lines;
	let landing_list: Vec<(u32, usize)> = flight_list
		.iter()
		.filter(|flight| flight.lane == lane)
		.map(|flight| (flight.land_ms, flight.frame))
		.collect();
	let unfed_change_list = (0..cells.len()).filter(|frame| {
		let dropped = lines(*frame) < frame.checked_sub(1).map_or(0, lines);
		let shown = cell_at(cells, *frame).visible;
		(dropped || shown) && landing_list.iter().all(|(_, fed)| fed != frame)
	});
	let mut event_list: Vec<(u32, bool, usize)> = landing_list
		.iter()
		.map(|(ms, frame)| (*ms, false, *frame))
		.chain(unfed_change_list.map(|frame| (frame_ms(clock, frame), true, frame)))
		.collect();
	event_list.sort_unstable();
	let mut landed: Vec<usize> = Vec::new();
	let mut step_list: Vec<(u32, u64)> = Vec::new();
	for (ms, is_frame_change, frame) in event_list {
		if !is_frame_change {
			landed.push(frame);
		}
		let current = frame_at(ms, clock, cells.len());
		let level = landed
			.iter()
			.filter(|fed| current.is_none_or(|current| **fed > current))
			.map(|fed| lines(*fed))
			.fold(current.map_or(0, lines), u64::max);
		let shown_level = step_list.last().map_or(0, |(_, last)| *last);
		let emits = if !is_frame_change {
			true
		} else if cell_at(cells, frame).visible {
			level != shown_level
		} else {
			level < shown_level
		};
		if emits {
			step_list.push((ms, level));
		}
	}
	step_list
}

fn frame_ms(clock: &Clock, frame: usize) -> u32 {
	u32::try_from(whole_ms(clock.ms(clock.lead + frame))).unwrap_or(u32::MAX)
}

fn frame_at(ms: u32, clock: &Clock, frames: usize) -> Option<usize> {
	usize::try_from(ms / clock.tick_ms)
		.unwrap_or(usize::MAX)
		.checked_sub(clock.lead)
		.map(|frame| frame.min(frames.saturating_sub(1)))
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

	fn landing(frame: usize, land_ms: u32) -> Flight {
		Flight {
			lane: 0,
			name: "Rust",
			frame,
			enter_ms: 0,
			fire_ms: 0,
			land_ms,
			to_x: 0.0,
			landing_y: 0.0,
		}
	}

	#[test]
	fn reentry_feeds_only_growth_over_the_previous_frame() {
		let cells = [
			cell(10, true),
			cell(10, true),
			cell(30, false),
			cell(30, true),
			cell(30, true),
			cell(40, true),
			cell(20, false),
			cell(25, true),
		];
		let feeds = lane_feeds(3, &cells);
		let frame_list: Vec<usize> = feeds.iter().map(|feed| feed.frame).collect();
		assert_eq!(frame_list, [0, 5, 7]);
		assert!(feeds.iter().all(|feed| feed.lane == 3));
		let fed_list: Vec<u64> = feeds.iter().map(|feed| feed.fed_before).collect();
		assert_eq!(fed_list, [0, 30, 20]);
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
	fn bar_steps_drop_at_frame_time_without_flight() {
		let clock = Clock::new(4, 300, 0);
		let cells = [
			cell(400, true),
			cell(400, true),
			cell(100, true),
			cell(100, true),
		];
		let flight_list = [
			landing(0, 1_500),
			Flight {
				lane: 1,
				..landing(1, 1_700)
			},
		];
		assert_eq!(
			bar_steps(0, &cells, &flight_list, &clock),
			[(1_500, 400), (frame_ms(&clock, 2), 100)]
		);
	}

	#[test]
	fn regrowth_after_drop_is_fed() {
		let clock = Clock::new(4, 300, 0);
		let cells = [
			cell(400, true),
			cell(100, true),
			cell(300, true),
			cell(300, true),
		];
		let feeds = lane_feeds(0, &cells);
		let fed: Vec<(usize, u64)> = feeds
			.iter()
			.map(|feed| (feed.frame, feed.fed_before))
			.collect();
		assert_eq!(fed, [(0, 0), (2, 100)]);
		let regrow_ms = frame_ms(&clock, 2) - 40;
		assert_eq!(
			bar_steps(
				0,
				&cells,
				&[landing(2, regrow_ms), landing(0, 1_500)],
				&clock
			),
			[(1_500, 400), (frame_ms(&clock, 1), 100), (regrow_ms, 300)]
		);
		let early_ms = frame_ms(&clock, 1);
		assert_eq!(
			bar_steps(
				0,
				&cells,
				&[landing(0, 1_500), landing(2, early_ms)],
				&clock
			),
			[(1_500, 400), (early_ms, 300)]
		);
	}

	#[test]
	fn hidden_frames_never_raise_the_bar() {
		let clock = Clock::new(4, 300, 0);
		let cells = [
			cell(10, true),
			cell(50, false),
			cell(40, false),
			cell(60, true),
		];
		let land_ms = frame_ms(&clock, 2) + 30;
		assert_eq!(
			bar_steps(0, &cells, &[landing(0, 1_500), landing(3, land_ms)], &clock),
			[(1_500, 10), (land_ms, 60)]
		);
		let shrunk = [
			cell(100, true),
			cell(150, false),
			cell(40, false),
			cell(60, true),
		];
		assert_eq!(
			bar_steps(
				0,
				&shrunk,
				&[landing(0, 1_500), landing(3, land_ms)],
				&clock
			),
			[(1_500, 100), (frame_ms(&clock, 2), 40), (land_ms, 60)]
		);
	}

	#[test]
	fn growth_while_hidden_shows_on_reentry_without_flight() {
		let clock = Clock::new(5, 400, 0);
		let cells = [
			cell(10, true),
			cell(10, false),
			cell(30, false),
			cell(30, false),
			cell(30, true),
		];
		assert_eq!(
			bar_steps(0, &cells, &[landing(0, 1_500)], &clock),
			[(1_500, 10), (frame_ms(&clock, 4), 30)]
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
