use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::clock::Clock;
use crate::timeline::Timeline;

const SWAP_REST_MS: u32 = 700;
const SETTLE_MS: f64 = 110.0;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Cell {
	pub(super) lines: u64,
	pub(super) share_bp: u32,
	pub(super) repos: u32,
	pub(super) rank: usize,
	pub(super) present: bool,
	pub(super) visible: bool,
	pub(super) slot: usize,
}

pub(super) fn grid(timeline: &Timeline, clock: &Clock) -> Vec<Vec<Cell>> {
	let mut grid = vec![vec![Cell::default(); timeline.frames.len()]; timeline.languages.len()];
	for (column, frame) in timeline.frames.iter().enumerate() {
		for (rank, entry) in frame.entries.iter().enumerate() {
			let target = grid
				.get_mut(entry.language)
				.and_then(|row| row.get_mut(column));
			if let Some(cell) = target {
				*cell = Cell {
					lines: entry.lines,
					share_bp: entry.share_bp,
					repos: entry.repos,
					rank,
					present: true,
					..Cell::default()
				};
			}
		}
		let absent = grid
			.iter_mut()
			.filter_map(|row| row.get_mut(column))
			.filter(|cell| !cell.present);
		for (rank, cell) in (frame.entries.len()..).zip(absent) {
			cell.rank = rank;
		}
	}
	let rest_frames = clock.ticks_in(SWAP_REST_MS);
	stabilize_ranks(&mut grid, rest_frames);
	hold_visibility(&mut grid, timeline.top_n, rest_frames);
	assign_slots(&mut grid);
	grid
}

fn hold_visibility(grid: &mut [Vec<Cell>], top_n: usize, rest_frames: usize) {
	let frames = grid.first().map_or(0, Vec::len);
	let mut shown = vec![false; grid.len()];
	let mut changed_at: Vec<Option<usize>> = vec![None; grid.len()];
	for column in 0..frames {
		let wanted: Vec<bool> = grid
			.iter()
			.map(|row| row[column].present && row[column].rank < top_n)
			.collect();
		if column == 0 || column + 1 == frames {
			shown.clone_from(&wanted);
		} else {
			let rank = |lane: &usize| grid[*lane][column].rank;
			let settled =
				|lane: &usize| changed_at[*lane].is_none_or(|at| at + rest_frames <= column);
			let mut incoming: Vec<usize> = (0..grid.len())
				.filter(|lane| wanted[*lane] && !shown[*lane] && settled(lane))
				.collect();
			incoming.sort_by_key(rank);
			let mut outgoing: Vec<usize> = (0..grid.len())
				.filter(|lane| shown[*lane] && !wanted[*lane] && settled(lane))
				.collect();
			outgoing.sort_by_key(|lane| Reverse(rank(lane)));
			let mut outgoing = outgoing.into_iter();
			for lane in incoming {
				if shown.iter().filter(|visible| **visible).count() >= top_n {
					let Some(leaving) = outgoing.next() else {
						break;
					};
					shown[leaving] = false;
					changed_at[leaving] = Some(column);
				}
				shown[lane] = true;
				changed_at[lane] = Some(column);
			}
		}
		for (row, visible) in grid.iter_mut().zip(&shown) {
			row[column].visible = *visible;
		}
	}
}

fn assign_slots(grid: &mut [Vec<Cell>]) {
	let frames = grid.first().map_or(0, Vec::len);
	for column in 0..frames {
		let mut visible: Vec<usize> = (0..grid.len())
			.filter(|lane| grid[*lane][column].visible)
			.collect();
		visible.sort_by_key(|lane| grid[*lane][column].rank);
		for row in grid.iter_mut() {
			row[column].slot = usize::MAX;
		}
		for (slot, lane) in visible.into_iter().enumerate() {
			grid[lane][column].slot = slot;
		}
	}
}

fn stabilize_ranks(grid: &mut [Vec<Cell>], rest_frames: usize) {
	let frames = grid.first().map_or(0, Vec::len);
	let mut order: Vec<usize> = Vec::new();
	let mut pair_free_from: BTreeMap<(usize, usize), usize> = BTreeMap::new();
	for column in 0..frames {
		if column == 0 || column + 1 == frames {
			order = (0..grid.len()).collect();
			order.sort_by_key(|index| grid[*index][column].rank);
		} else {
			let lines: Vec<u64> = grid.iter().map(|row| row[column].lines).collect();
			let swapped = overtake(&mut order, &lines, |pair| {
				pair_free_from
					.get(&pair)
					.is_none_or(|free_from| *free_from <= column)
			});
			for pair in swapped {
				pair_free_from.insert(pair, column + rest_frames);
			}
		}
		for (rank, index) in order.iter().enumerate() {
			grid[*index][column].rank = rank;
		}
	}
}

fn overtake(
	order: &mut [usize],
	lines: &[u64],
	is_free: impl Fn((usize, usize)) -> bool,
) -> Vec<(usize, usize)> {
	let mut swapped = Vec::new();
	for _ in 0..order.len() {
		for position in 1..order.len() {
			let (ahead, behind) = (order[position - 1], order[position]);
			let pair = (ahead.min(behind), ahead.max(behind));
			if lines[behind] > lines[ahead] && (lines[ahead] == 0 || is_free(pair)) {
				order.swap(position - 1, position);
				swapped.push(pair);
			}
		}
	}
	swapped
}

pub(super) fn cell_at(cells: &[Cell], frame: usize) -> Cell {
	cells.get(frame).copied().unwrap_or_default()
}

pub(super) fn settle(target_list: &[f64], tick_ms: f64) -> Vec<f64> {
	let pull = tick_ms / (tick_ms + SETTLE_MS);
	let last = target_list.len().saturating_sub(1);
	let mut position = target_list.first().copied().unwrap_or(0.0);
	target_list
		.iter()
		.enumerate()
		.map(|(tick, target)| {
			position = (target - position).mul_add(pull, position);
			let settled = if tick == last { *target } else { position };
			settled.round()
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::language::Language;
	use crate::render::clock::race_ms;
	use crate::timeline::{Entry, Frame, LanguageSummary, Trend};
	use jiff::Span;
	use jiff::civil::Date;

	const NAME_LIST: [&str; 5] = ["A", "B", "C", "D", "E"];

	fn race(lines_per_frame: &[Vec<u64>]) -> Timeline {
		let count = lines_per_frame.first().map_or(0, Vec::len);
		let start = Date::constant(2020, 1, 6);
		let languages = NAME_LIST[..count]
			.iter()
			.map(|name| LanguageSummary {
				language: Language {
					name,
					color: "#000000",
				},
				since: start,
				trend: Trend {
					now: 0,
					year_ago: 0,
				},
			})
			.collect();
		let frames = (0_i64..)
			.zip(lines_per_frame)
			.map(|(index, lines)| {
				let mut entries: Vec<Entry> = (0..count)
					.filter(|language| lines[*language] > 0)
					.map(|language| Entry {
						language,
						lines: lines[language],
						share_bp: 0,
						repos: 0,
					})
					.collect();
				entries.sort_by_key(|entry| (std::cmp::Reverse(entry.lines), entry.language));
				Frame {
					week: start.saturating_add(Span::new().weeks(index)),
					entries,
					language_count: 0,
					total_lines: 0,
				}
			})
			.collect();
		Timeline {
			languages,
			frames,
			top_n: count,
		}
	}

	fn race_within(lines_per_frame: &[Vec<u64>], top_n: usize) -> Timeline {
		Timeline {
			top_n,
			..race(lines_per_frame)
		}
	}

	fn visible(timeline: &Timeline, language: usize) -> Vec<bool> {
		grid(timeline, &clock_for(timeline))[language]
			.iter()
			.map(|cell| cell.visible)
			.collect()
	}

	fn change_count(list: &[bool]) -> usize {
		list.windows(2).filter(|pair| pair[0] != pair[1]).count()
	}

	fn clock_for(timeline: &Timeline) -> Clock {
		Clock::new(timeline.frames.len(), race_ms(timeline, 5.0), 0)
	}

	fn ranks(timeline: &Timeline, language: usize) -> Vec<usize> {
		let clock = clock_for(timeline);
		grid(timeline, &clock)[language]
			.iter()
			.map(|cell| cell.rank)
			.collect()
	}

	fn true_ranks(timeline: &Timeline, language: usize) -> Vec<usize> {
		timeline
			.frames
			.iter()
			.map(|frame| {
				frame
					.entries
					.iter()
					.position(|entry| entry.language == language)
					.unwrap()
			})
			.collect()
	}

	fn rest_frames(timeline: &Timeline) -> usize {
		clock_for(timeline).ticks_in(SWAP_REST_MS)
	}

	fn alternating(frame_count: u64) -> Timeline {
		let lines: Vec<Vec<u64>> = (0..frame_count)
			.map(|frame| vec![1_000 + frame * 10 + frame % 2 * 15, 1_005 + frame * 10])
			.collect();
		race(&lines)
	}

	#[test]
	fn first_and_last_frames_use_true_order() {
		let timeline = alternating(41);
		let shown = ranks(&timeline, 0);
		let truth = true_ranks(&timeline, 0);
		assert_eq!(shown.first(), truth.first());
		assert_eq!(shown.last(), truth.last());
	}

	#[test]
	fn near_tie_swaps_are_rate_limited() {
		let timeline = alternating(100);
		let shown = ranks(&timeline, 0);
		let change_count = shown.windows(2).filter(|pair| pair[0] != pair[1]).count();
		let truth_change_count = true_ranks(&timeline, 0)
			.windows(2)
			.filter(|pair| pair[0] != pair[1])
			.count();
		assert!(truth_change_count >= 90);
		assert!(change_count >= 2, "{shown:?}");
		assert!(rest_frames(&timeline) >= 2);
		assert!(
			change_count <= 100 / rest_frames(&timeline) + 1,
			"{change_count} {shown:?}"
		);
	}

	#[test]
	fn steady_small_lead_is_shown_within_rest_bound() {
		let lines: Vec<Vec<u64>> = (0..80_u64)
			.map(|frame| match frame {
				0..10 => vec![100, 200],
				10..12 => vec![300, 200],
				_ => vec![1_000 + frame, 1_050 + frame],
			})
			.collect();
		let timeline = race(&lines);
		let rest = rest_frames(&timeline);
		let clock = clock_for(&timeline);
		assert!(f64::from(clock.tick_ms) * f64::from(u32::try_from(rest).unwrap()) <= 1_000.0);
		let shown = ranks(&timeline, 1);
		assert_eq!(shown[11], 1);
		for (frame, rank) in shown.iter().enumerate().skip(10 + rest) {
			assert_eq!(*rank, 0, "frame {frame}: {shown:?}");
		}
	}

	#[test]
	fn multi_language_pass_happens_at_once() {
		let lines: Vec<Vec<u64>> = (0..20_u64)
			.map(|frame| {
				let rising = if frame < 5 { 10 } else { 5_000 };
				vec![400, 300, 200, rising]
			})
			.collect();
		let timeline = race(&lines);
		let shown = ranks(&timeline, 3);
		assert_eq!(shown[4], 3);
		assert!(shown[5..].iter().all(|rank| *rank == 0), "{shown:?}");
		assert_eq!(ranks(&timeline, 0)[5], 1);
		assert_eq!(ranks(&timeline, 2)[5], 3);
	}

	#[test]
	fn new_language_enters_at_its_rank() {
		let lines: Vec<Vec<u64>> = (0..20_u64)
			.map(|frame| {
				let late = if frame < 5 { 0 } else { 250 };
				vec![400, 300, 200, late]
			})
			.collect();
		let timeline = race(&lines);
		let late = grid(&timeline, &clock_for(&timeline)).swap_remove(3);
		assert!(!late[4].present);
		assert!(late[5].present);
		assert!(late[5..].iter().all(|cell| cell.rank == 2));
		assert_eq!(ranks(&timeline, 2)[5], 3);
	}

	#[test]
	fn settle_eases_toward_target_and_snaps_at_the_end() {
		let x_list = settle(&[0.0, 100.0, 100.0, 100.0], 100.0);
		assert_eq!(x_list[0], 0.0);
		assert!(x_list[1] > 0.0 && x_list[1] < 100.0);
		assert_eq!(x_list[3], 100.0);
	}

	#[test]
	fn boundary_crossing_hides_then_shows_again() {
		let lines: Vec<Vec<u64>> = (0..60_u64)
			.map(|frame| {
				let wanderer = match frame {
					0..15 => 500,
					15..30 => 100,
					30..45 => 900,
					_ => 50,
				};
				vec![1_000, wanderer, 300]
			})
			.collect();
		let timeline = race_within(&lines, 2);
		let wanderer = visible(&timeline, 1);
		let rest = rest_frames(&timeline);
		assert!(wanderer[..15].iter().all(|shown| *shown));
		assert!(
			wanderer[15 + rest..30].iter().all(|shown| !*shown),
			"{wanderer:?}"
		);
		assert!(
			wanderer[30 + rest..45].iter().all(|shown| *shown),
			"{wanderer:?}"
		);
		assert!(!wanderer[59]);
		assert_eq!(change_count(&wanderer), 3);
		let steady = visible(&timeline, 2);
		assert_eq!(change_count(&steady), 3);
		assert!(visible(&timeline, 0).iter().all(|shown| *shown));
	}

	#[test]
	fn at_most_top_n_visible_each_frame() {
		let lines: Vec<Vec<u64>> = (0..120_u64)
			.map(|frame| {
				(0..5_u64)
					.map(|language| {
						let start = language * 9;
						frame.saturating_sub(start) * (1 + (frame * 7 + language * 13) % 5) * 10
					})
					.collect()
			})
			.collect();
		let timeline = race_within(&lines, 3);
		let clock = clock_for(&timeline);
		let grid = grid(&timeline, &clock);
		for frame in 0..timeline.frames.len() {
			let shown = grid.iter().filter(|row| row[frame].visible).count();
			assert!(shown <= 3, "frame {frame}");
		}
		let last = timeline.frames.len() - 1;
		for (rank, entry) in timeline.frames[last].entries.iter().enumerate() {
			assert_eq!(grid[entry.language][last].visible, rank < 3);
		}
		assert!(!grid[4][0].visible);
		for frame in 0..timeline.frames.len() {
			let mut slot_list: Vec<usize> = grid
				.iter()
				.filter(|row| row[frame].visible)
				.map(|row| row[frame].slot)
				.collect();
			slot_list.sort_unstable();
			let expected: Vec<usize> = (0..slot_list.len()).collect();
			assert_eq!(slot_list, expected, "frame {frame}");
		}
	}

	fn change_frames(list: &[bool]) -> Vec<usize> {
		(1..list.len())
			.filter(|frame| list[frame - 1] != list[*frame])
			.collect()
	}

	#[test]
	fn visibility_holds_when_two_different_pairs_cross_the_boundary() {
		let lines: Vec<Vec<u64>> = (0..40_u64)
			.map(|frame| match frame {
				0..10 => vec![1_000, 900, 800],
				10 => vec![1_000, 940, 950],
				_ => vec![1_000, 1_010 + frame, 1_020 + frame],
			})
			.collect();
		let timeline = race_within(&lines, 2);
		let rest = rest_frames(&timeline);
		assert!(rest >= 2);
		assert_eq!(ranks(&timeline, 1)[10..12], [2, 1]);
		for language in 0..3 {
			let change_list = change_frames(&visible(&timeline, language));
			assert!(
				change_list.windows(2).all(|pair| pair[1] - pair[0] >= rest),
				"{language} {change_list:?}"
			);
		}
		let grid = grid(&timeline, &clock_for(&timeline));
		for frame in 0..timeline.frames.len() {
			assert!(grid.iter().filter(|row| row[frame].visible).count() <= 2);
		}
		let x = visible(&timeline, 1);
		assert!(!x[10] && !x[11], "{x:?}");
		assert_eq!(change_frames(&x), [10, 10 + rest]);
	}

	#[test]
	fn boundary_flapping_is_held_by_the_swap_rest() {
		let lines: Vec<Vec<u64>> = (0..100_u64)
			.map(|frame| {
				vec![
					5_000 + frame * 10,
					1_000 + frame * 10 + frame % 2 * 15,
					1_005 + frame * 10,
				]
			})
			.collect();
		let timeline = race_within(&lines, 2);
		let flapping = visible(&timeline, 1);
		let rest = rest_frames(&timeline);
		assert!(rest >= 2);
		assert!(change_count(&flapping) >= 2, "{flapping:?}");
		assert!(change_count(&flapping) <= 100 / rest + 1, "{flapping:?}");
		let either: Vec<bool> = visible(&timeline, 2)
			.iter()
			.zip(&flapping)
			.map(|(other, flapper)| other ^ flapper)
			.collect();
		assert!(either.iter().all(|exactly_one| *exactly_one));
	}
}
