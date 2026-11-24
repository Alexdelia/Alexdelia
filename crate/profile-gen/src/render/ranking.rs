use std::collections::BTreeMap;

use super::clock::Clock;
use crate::timeline::Timeline;

const SWAP_REST_MS: u32 = 700;
const SETTLE_MS: f64 = 110.0;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Cell {
	pub(super) lines: u64,
	pub(super) share_bp: u32,
	pub(super) repos: Option<u32>,
	pub(super) rank: usize,
	pub(super) present: bool,
}

pub(super) fn grid(timeline: &Timeline, clock: &Clock) -> Vec<Vec<Cell>> {
	let mut grid = vec![vec![Cell::default(); timeline.frames.len()]; timeline.languages.len()];
	for (column, frame) in timeline.frames.iter().enumerate() {
		for (rank, entry) in frame.entries.iter().enumerate() {
			let slot = grid
				.get_mut(entry.language)
				.and_then(|row| row.get_mut(column));
			if let Some(cell) = slot {
				*cell = Cell {
					lines: entry.lines,
					share_bp: entry.share_bp,
					repos: entry.repos,
					rank,
					present: true,
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
	stabilize_ranks(&mut grid, clock.ticks_in(SWAP_REST_MS));
	grid
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
					last_year: 0,
					previous_year: 0,
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
						repos: None,
					})
					.collect();
				entries.sort_by_key(|entry| (std::cmp::Reverse(entry.lines), entry.language));
				Frame {
					week: start.saturating_add(Span::new().weeks(index)),
					entries,
				}
			})
			.collect();
		Timeline { languages, frames }
	}

	fn ranks(timeline: &Timeline, language: usize) -> Vec<usize> {
		let clock = Clock::new(timeline.frames.len());
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
		Clock::new(timeline.frames.len()).ticks_in(SWAP_REST_MS)
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
		let clock = Clock::new(timeline.frames.len());
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
		let late = grid(&timeline, &Clock::new(20)).swap_remove(3);
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
}
