use std::cmp::Reverse;
use std::collections::BTreeSet;

use jiff::Span;

use crate::cache::Usage;
use crate::history::{Deltas, Week};
use crate::language::{Language, language_by_name};

const BASIS_POINTS: u64 = 10_000;
const TREND_WEEKS: usize = 52;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
	Up,
	Down,
	Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trend {
	pub now: u64,
	pub year_ago: u64,
}

impl Trend {
	#[must_use]
	pub const fn direction(&self) -> Direction {
		if self.now * 10 > self.year_ago * 11 || (self.year_ago == 0 && self.now > 0) {
			Direction::Up
		} else if self.now * 10 < self.year_ago * 9 {
			Direction::Down
		} else {
			Direction::Flat
		}
	}

	#[must_use]
	pub fn change(&self) -> i64 {
		let signed = |lines: u64| i64::try_from(lines).unwrap_or(i64::MAX);
		signed(self.now).saturating_sub(signed(self.year_ago))
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSummary {
	pub language: Language,
	pub since: Week,
	pub trend: Trend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
	pub language: usize,
	pub lines: u64,
	pub share_bp: u32,
	pub repos: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
	pub week: Week,
	pub entries: Vec<Entry>,
	pub language_count: u32,
	pub total_lines: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
	pub languages: Vec<LanguageSummary>,
	pub frames: Vec<Frame>,
	pub top_n: usize,
}

struct Series {
	language: Language,
	levels: Vec<u64>,
	repos: Vec<u32>,
}

struct Totals {
	language_count: u32,
	total_lines: u64,
}

#[must_use]
pub fn timeline(usage: &Usage, top_n: usize, max_frames: usize) -> Timeline {
	let weeks = week_range(usage);
	if weeks.is_empty() {
		return Timeline::default();
	}
	let names: BTreeSet<&'static str> = usage
		.lines
		.values()
		.flat_map(|bucket| bucket.keys().copied())
		.collect();
	let all_levels: Vec<Vec<u64>> = names
		.iter()
		.map(|name| levels(&usage.lines, name, &weeks))
		.collect();
	let known = known_series(usage, &names, &all_levels, &weeks);
	let frame_indices = sample_indices(weeks.len(), max_frames, peak_index(&known));
	let shown = within_top_n_somewhere(&known, &frame_indices, top_n);
	let series_list: Vec<&Series> = shown.iter().map(|index| &known[*index]).collect();
	let languages: Vec<LanguageSummary> = series_list
		.iter()
		.map(|series| summary(series, &weeks))
		.collect();
	let frames = frame_indices
		.into_iter()
		.map(|index| {
			let totals = Totals {
				language_count: u32::try_from(
					all_levels.iter().filter(|level| level[index] > 0).count(),
				)
				.unwrap_or(u32::MAX),
				total_lines: all_levels.iter().map(|level| level[index]).sum(),
			};
			frame(weeks[index], index, &totals, &series_list, &languages)
		})
		.collect();
	Timeline {
		languages,
		frames,
		top_n,
	}
}

fn week_range(usage: &Usage) -> Vec<Week> {
	let first = usage
		.lines
		.iter()
		.find(|(_, bucket)| bucket.values().any(|delta| *delta > 0))
		.map(|(week, _)| *week);
	let Some(first) = first else {
		return Vec::new();
	};
	let last = usage.lines.keys().next_back().copied().unwrap_or(first);
	std::iter::successors(Some(first), |week| {
		Some(week.saturating_add(Span::new().weeks(1)))
	})
	.take_while(|week| *week <= last)
	.collect()
}

fn levels(deltas: &Deltas, name: &str, weeks: &[Week]) -> Vec<u64> {
	weeks
		.iter()
		.scan(0_i64, |level, week| {
			*level += deltas
				.get(week)
				.and_then(|bucket| bucket.get(name))
				.copied()
				.unwrap_or(0);
			Some(u64::try_from(*level).unwrap_or(0))
		})
		.collect()
}

fn known_series(
	usage: &Usage,
	names: &BTreeSet<&'static str>,
	all_levels: &[Vec<u64>],
	weeks: &[Week],
) -> Vec<Series> {
	let mut known: Vec<Series> = names
		.iter()
		.zip(all_levels)
		.filter(|(_, levels)| levels.iter().any(|level| *level > 0))
		.filter_map(|(name, levels)| {
			language_by_name(name).map(|language| Series {
				language,
				levels: levels.clone(),
				repos: levels_of_repos(&usage.repos, name, weeks),
			})
		})
		.collect();
	known.sort_by_key(|series| {
		(
			Reverse(series.levels.last().copied().unwrap_or(0)),
			Reverse(series.levels.iter().max().copied().unwrap_or(0)),
			series.language.name,
		)
	});
	known
}

fn levels_of_repos(repos: &Deltas, name: &str, weeks: &[Week]) -> Vec<u32> {
	levels(repos, name, weeks)
		.into_iter()
		.map(|count| u32::try_from(count).unwrap_or(u32::MAX))
		.collect()
}

fn peak_index(known: &[Series]) -> usize {
	known
		.iter()
		.flat_map(|series| series.levels.iter().copied().enumerate())
		.max_by_key(|(index, level)| (*level, Reverse(*index)))
		.map_or(0, |(index, _)| index)
}

fn within_top_n_somewhere(known: &[Series], frame_indices: &[usize], top_n: usize) -> Vec<usize> {
	let mut shown = vec![false; known.len()];
	for index in frame_indices {
		let mut ranked: Vec<usize> = (0..known.len())
			.filter(|known_index| known[*known_index].levels[*index] > 0)
			.collect();
		ranked.sort_by_key(|known_index| {
			(
				Reverse(known[*known_index].levels[*index]),
				known[*known_index].language.name,
			)
		});
		for known_index in ranked.into_iter().take(top_n) {
			shown[known_index] = true;
		}
	}
	(0..known.len()).filter(|index| shown[*index]).collect()
}

fn summary(series: &Series, weeks: &[Week]) -> LanguageSummary {
	let levels = &series.levels;
	let since = levels
		.iter()
		.position(|level| *level > 0)
		.map_or(weeks[0], |index| weeks[index]);
	let last = levels.len() - 1;
	LanguageSummary {
		language: series.language,
		since,
		trend: Trend {
			now: levels[last],
			year_ago: last
				.checked_sub(TREND_WEEKS)
				.map_or(0, |index| levels[index]),
		},
	}
}

fn sample_indices(count: usize, max_frames: usize, keep: usize) -> Vec<usize> {
	let target = max_frames.max(2);
	if count <= target {
		return (0..count).collect();
	}
	let mut indices: Vec<usize> = (0..target)
		.map(|step| step * (count - 1) / (target - 1))
		.collect();
	if indices.binary_search(&keep).is_err() {
		let interior = 1..target - 1;
		if let Some(slot) = interior.min_by_key(|slot| indices[*slot].abs_diff(keep)) {
			indices[slot] = keep;
		} else {
			indices.push(keep);
			indices.sort_unstable();
		}
	}
	indices
}

fn frame(
	week: Week,
	index: usize,
	totals: &Totals,
	series_list: &[&Series],
	languages: &[LanguageSummary],
) -> Frame {
	let lines: Vec<u64> = series_list
		.iter()
		.map(|series| series.levels[index])
		.collect();
	let present: Vec<usize> = (0..lines.len()).filter(|shown| lines[*shown] > 0).collect();
	let mut counted: Vec<u64> = present.iter().map(|shown| lines[*shown]).collect();
	counted.push(totals.total_lines - lines.iter().sum::<u64>());
	let mut entries: Vec<Entry> = present
		.iter()
		.zip(shares(&counted))
		.map(|(shown, share_bp)| Entry {
			language: *shown,
			lines: lines[*shown],
			share_bp,
			repos: series_list[*shown].repos[index],
		})
		.collect();
	entries.sort_by_key(|entry| {
		(
			Reverse(entry.lines),
			languages[entry.language].language.name,
		)
	});
	Frame {
		week,
		entries,
		language_count: totals.language_count,
		total_lines: totals.total_lines,
	}
}

fn shares(lines: &[u64]) -> Vec<u32> {
	let total = u128::from(lines.iter().sum::<u64>()).max(1);
	let scaled: Vec<u128> = lines
		.iter()
		.map(|count| u128::from(*count) * u128::from(BASIS_POINTS))
		.collect();
	let mut share_list: Vec<u64> = scaled
		.iter()
		.map(|value| u64::try_from(value / total).unwrap_or(BASIS_POINTS))
		.collect();
	let leftover = BASIS_POINTS - share_list.iter().sum::<u64>();
	let mut order: Vec<usize> = (0..lines.len()).collect();
	order.sort_by_key(|index| (Reverse(scaled[*index] % total), *index));
	for (index, _) in order.into_iter().zip(0..leftover) {
		share_list[index] += 1;
	}
	share_list
		.into_iter()
		.map(|share| u32::try_from(share).unwrap_or(u32::MAX))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use jiff::civil::Date;
	use std::collections::BTreeMap;

	fn week(index: i64) -> Week {
		Date::constant(2020, 1, 6).saturating_add(Span::new().weeks(index))
	}

	fn add(deltas: &mut Deltas, entries: &[(i64, &'static str, i64)]) {
		for (index, language, delta) in entries {
			*deltas
				.entry(week(*index))
				.or_default()
				.entry(language)
				.or_default() += delta;
		}
	}

	fn usage(lines: &[(i64, &'static str, i64)], repos: &[(i64, &'static str, i64)]) -> Usage {
		let mut usage = Usage::default();
		add(&mut usage.lines, lines);
		add(&mut usage.repos, repos);
		usage
	}

	fn names(timeline: &Timeline) -> Vec<&'static str> {
		timeline
			.languages
			.iter()
			.map(|summary| summary.language.name)
			.collect()
	}

	fn find<'a>(timeline: &'a Timeline, frame: usize, name: &str) -> Option<&'a Entry> {
		timeline.frames[frame]
			.entries
			.iter()
			.find(|entry| timeline.languages[entry.language].language.name == name)
	}

	fn lines_of(timeline: &Timeline, frame: usize, name: &str) -> Option<u64> {
		find(timeline, frame, name).map(|entry| entry.lines)
	}

	fn series_of(timeline: &Timeline, name: &str) -> Vec<Option<u64>> {
		(0..timeline.frames.len())
			.map(|frame| lines_of(timeline, frame, name))
			.collect()
	}

	fn order(timeline: &Timeline, frame: usize) -> Vec<&'static str> {
		timeline.frames[frame]
			.entries
			.iter()
			.map(|entry| timeline.languages[entry.language].language.name)
			.collect()
	}

	fn sample() -> Usage {
		usage(
			&[
				(0, "Rust", 100),
				(1, "Rust", 50),
				(1, "C", 10),
				(2, "C", 300),
				(3, "Rust", -30),
			],
			&[],
		)
	}

	#[test]
	fn level_is_running_sum_of_deltas() {
		let result = timeline(&sample(), 10, 100);
		assert_eq!(result.frames.len(), 4);
		assert_eq!(
			series_of(&result, "Rust"),
			[Some(100), Some(150), Some(150), Some(120)]
		);
		assert_eq!(
			series_of(&result, "C"),
			[None, Some(10), Some(310), Some(310)]
		);
		assert_eq!(names(&result), ["C", "Rust"]);
	}

	#[test]
	fn lanes_order_by_final_level_then_peak_then_name() {
		let usage = usage(
			&[
				(0, "Go", 50),
				(0, "Nix", 80),
				(0, "Lua", 80),
				(0, "C", 10),
				(0, "Rust", 5),
				(1, "Go", -50),
				(1, "Nix", -80),
				(1, "Lua", -80),
			],
			&[],
		);
		let result = timeline(&usage, 10, 100);
		assert_eq!(names(&result), ["C", "Rust", "Lua", "Nix", "Go"]);
	}

	#[test]
	fn language_at_zero_leaves_frame() {
		let usage = usage(
			&[
				(0, "Rust", 10),
				(0, "C", 5),
				(1, "Rust", -10),
				(2, "Rust", 4),
				(3, "Rust", -4),
				(3, "C", -5),
				(4, "C", 1),
			],
			&[],
		);
		let result = timeline(&usage, 10, 100);
		assert_eq!(order(&result, 0), ["Rust", "C"]);
		assert_eq!(order(&result, 1), ["C"]);
		assert_eq!(find(&result, 1, "C").unwrap().share_bp, 10_000);
		assert_eq!(order(&result, 2), ["C", "Rust"]);
		assert_eq!(order(&result, 3), Vec::<&str>::new());
		assert_eq!(result.frames[3].total_lines, 0);
		assert_eq!(order(&result, 4), ["C"]);
	}

	#[test]
	fn shares_sum_to_10000_per_frame() {
		let usage = usage(
			&[(0, "Rust", 1), (0, "C", 1), (0, "Go", 1), (1, "Rust", 7)],
			&[],
		);
		let result = timeline(&usage, 10, 100);
		for frame in &result.frames {
			let total: u32 = frame.entries.iter().map(|entry| entry.share_bp).sum();
			assert_eq!(total, 10_000);
		}
		let first: Vec<_> = result.frames[0]
			.entries
			.iter()
			.map(|entry| {
				(
					result.languages[entry.language].language.name,
					entry.share_bp,
				)
			})
			.collect();
		assert_eq!(first, [("C", 3333), ("Go", 3333), ("Rust", 3334)]);
	}

	#[test]
	fn entries_race_sorted_by_share_each_frame() {
		let result = timeline(&sample(), 10, 100);
		assert_eq!(order(&result, 0), ["Rust"]);
		assert_eq!(order(&result, 1), ["Rust", "C"]);
		assert_eq!(order(&result, 2), ["C", "Rust"]);
		for frame in &result.frames {
			assert!(
				frame
					.entries
					.windows(2)
					.all(|pair| pair[0].share_bp >= pair[1].share_bp)
			);
		}
	}

	#[test]
	fn repos_follow_presence() {
		let usage = usage(
			&[(0, "Rust", 100), (1, "C", 10), (3, "Rust", 5)],
			&[(0, "Rust", 1), (1, "C", 1), (2, "Rust", 1), (3, "Rust", -1)],
		);
		let result = timeline(&usage, 10, 100);
		let repos = |frame: usize, name: &str| find(&result, frame, name).map(|entry| entry.repos);
		let rust: Vec<_> = (0..4).map(|frame| repos(frame, "Rust")).collect();
		assert_eq!(rust, [Some(1), Some(1), Some(2), Some(1)]);
		assert_eq!(repos(1, "C"), Some(1));
		let unlisted = timeline(&self::usage(&[(0, "Go", 5)], &[]), 10, 100);
		assert_eq!(unlisted.frames[0].entries[0].repos, 0);
	}

	fn summary_of(timeline: &Timeline, name: &str) -> LanguageSummary {
		timeline
			.languages
			.iter()
			.find(|summary| summary.language.name == name)
			.unwrap()
			.clone()
	}

	#[test]
	fn since_is_first_week_above_zero() {
		let usage = usage(
			&[
				(0, "Rust", 10),
				(2, "Go", 5),
				(5, "Go", -5),
				(7, "Go", 3),
				(4, "Nix", 0),
				(6, "Nix", 2),
			],
			&[],
		);
		let result = timeline(&usage, 10, 100);
		assert_eq!(summary_of(&result, "Rust").since, week(0));
		assert_eq!(summary_of(&result, "Go").since, week(2));
		assert_eq!(summary_of(&result, "Nix").since, week(6));
	}

	#[test]
	fn trend_compares_now_with_a_year_ago() {
		let lines = [
			(0, "Rust", 100),
			(60, "Rust", 100),
			(100, "Rust", 1),
			(0, "Go", 1000),
			(60, "Go", -500),
			(0, "Python", 100),
			(60, "Python", 5),
			(80, "Nix", 5),
		];
		let result = timeline(&usage(&lines, &[]), 10, 1000);
		assert_eq!(result.frames.len(), 101);
		let rust = summary_of(&result, "Rust");
		assert_eq!(
			rust.trend,
			Trend {
				now: 201,
				year_ago: 100
			}
		);
		assert_eq!(rust.trend.direction(), Direction::Up);
		assert_eq!(rust.trend.change(), 101);
		let go = summary_of(&result, "Go").trend;
		assert_eq!((go.direction(), go.change()), (Direction::Down, -500));
		let python = summary_of(&result, "Python").trend;
		assert_eq!((python.direction(), python.change()), (Direction::Flat, 5));
		let nix = summary_of(&result, "Nix").trend;
		assert_eq!(
			nix,
			Trend {
				now: 5,
				year_ago: 0
			}
		);
		assert_eq!(nix.direction(), Direction::Up);

		let young = timeline(&usage(&[(0, "Rust", 5), (10, "Rust", 1)], &[]), 10, 100);
		assert_eq!(
			summary_of(&young, "Rust").trend,
			Trend {
				now: 6,
				year_ago: 0
			}
		);
	}

	#[test]
	fn direction_thresholds() {
		let direction = |now, year_ago| Trend { now, year_ago }.direction();
		assert_eq!(direction(110, 100), Direction::Flat);
		assert_eq!(direction(111, 100), Direction::Up);
		assert_eq!(direction(90, 100), Direction::Flat);
		assert_eq!(direction(89, 100), Direction::Down);
		assert_eq!(direction(0, 0), Direction::Flat);
		assert_eq!(direction(1, 0), Direction::Up);
		assert_eq!(direction(0, 5), Direction::Down);
		let huge = Trend {
			now: 0,
			year_ago: u64::MAX,
		};
		assert_eq!(huge.change(), -i64::MAX);
	}

	#[test]
	fn scale_input_peak_survives_later_drop() {
		let mut lines: Vec<(i64, &'static str, i64)> =
			(0..=137).map(|index| (index, "Rust", 10)).collect();
		lines.extend((138..340).map(|index| (index, "Rust", -1)));
		lines.push((0, "C", 3));
		let usage = usage(&lines, &[]);
		let peak = |result: &Timeline| {
			result
				.frames
				.iter()
				.flat_map(|frame| frame.entries.iter().map(|entry| entry.lines))
				.max()
		};
		let sampled = timeline(&usage, 10, 150);
		assert_eq!(sampled.frames.len(), 150);
		assert_eq!(peak(&sampled), Some(1380));
		assert!(sampled.frames.iter().any(|frame| frame.week == week(137)));
		assert!(
			sampled
				.frames
				.windows(2)
				.all(|pair| pair[0].week < pair[1].week)
		);
		let tiny = timeline(&usage, 10, 2);
		let weeks: Vec<_> = tiny.frames.iter().map(|frame| frame.week).collect();
		assert_eq!(weeks, [week(0), week(137), week(339)]);
	}

	fn five_languages() -> Usage {
		usage(
			&[
				(0, "Rust", 500),
				(0, "C", 400),
				(0, "Go", 300),
				(2, "Nix", 200),
				(1, "Python", 100),
			],
			&[(0, "Rust", 1), (2, "Nix", 1), (1, "Python", 1)],
		)
	}

	fn share_of(timeline: &Timeline, frame: usize, name: &str) -> Option<u32> {
		find(timeline, frame, name).map(|entry| entry.share_bp)
	}

	#[test]
	fn languages_never_within_top_n_are_not_drawn_but_counted() {
		let result = timeline(&five_languages(), 3, 100);
		assert_eq!(names(&result), ["Rust", "C", "Go"]);
		assert_eq!(result.top_n, 3);
		let last = result.frames.len() - 1;
		assert_eq!(share_of(&result, last, "Rust"), Some(3333));
		assert_eq!(share_of(&result, last, "Go"), Some(2000));
		let shown: u32 = result.frames[last]
			.entries
			.iter()
			.map(|entry| entry.share_bp)
			.sum();
		assert_eq!(shown, 8000);
		assert!(!names(&result).contains(&"other"));
	}

	#[test]
	fn language_within_top_n_at_any_frame_is_a_candidate() {
		let usage = usage(
			&[
				(0, "Go", 100),
				(0, "Rust", 50),
				(0, "Nix", 10),
				(1, "C", 200),
				(2, "Python", 300),
			],
			&[],
		);
		let result = timeline(&usage, 2, 100);
		assert_eq!(names(&result), ["Python", "C", "Go", "Rust"]);
		assert_eq!(order(&result, 0), ["Go", "Rust"]);
		assert_eq!(order(&result, 1), ["C", "Go", "Rust"]);
		assert_eq!(order(&result, 2), ["Python", "C", "Go", "Rust"]);
		assert_eq!(share_of(&result, 2, "Rust"), Some(758));
	}

	#[test]
	fn all_languages_shown_within_top_n() {
		let result = timeline(&five_languages(), 5, 100);
		assert_eq!(names(&result), ["Rust", "C", "Go", "Nix", "Python"]);
		let result = timeline(&five_languages(), 9, 100);
		assert_eq!(result.languages.len(), 5);
	}

	#[test]
	fn unknown_language_is_hidden_but_counted() {
		let usage = usage(&[(0, "Rust", 10), (0, "Unknown", 100)], &[]);
		let result = timeline(&usage, 1, 100);
		assert_eq!(names(&result), ["Rust"]);
		assert_eq!(share_of(&result, 0, "Rust"), Some(909));
	}

	fn totals(timeline: &Timeline) -> Vec<(u32, u64)> {
		timeline
			.frames
			.iter()
			.map(|frame| (frame.language_count, frame.total_lines))
			.collect()
	}

	fn rising_and_falling() -> Usage {
		usage(
			&[
				(0, "Rust", 10),
				(0, "Unknown", 5),
				(1, "Go", 3),
				(2, "Rust", -10),
				(3, "Unknown", -5),
			],
			&[],
		)
	}

	#[test]
	fn language_count_counts_languages_above_zero() {
		let counts: Vec<u32> = totals(&timeline(&rising_and_falling(), 1, 100))
			.into_iter()
			.map(|(count, _)| count)
			.collect();
		assert_eq!(counts, [2, 3, 2, 1]);
		let result = timeline(&five_languages(), 3, 100);
		assert_eq!(totals(&result), [(3, 1200), (4, 1300), (5, 1500)]);
	}

	#[test]
	fn total_lines_is_existing_total() {
		let total_list: Vec<u64> = totals(&timeline(&rising_and_falling(), 1, 100))
			.into_iter()
			.map(|(_, total)| total)
			.collect();
		assert_eq!(total_list, [15, 18, 8, 3]);
	}

	#[test]
	fn totals_hold_through_gap_weeks() {
		let usage = usage(&[(0, "Rust", 10), (3, "C", 5), (3, "Rust", 0)], &[]);
		assert_eq!(
			totals(&timeline(&usage, 10, 100)),
			[(1, 10), (1, 10), (1, 10), (2, 15)]
		);
	}

	#[test]
	fn resampled_totals_match_their_week() {
		let lines: Vec<(i64, &'static str, i64)> = (0..340)
			.flat_map(|index| {
				let late = if index >= 200 {
					vec![(index, "Go", 3)]
				} else {
					Vec::new()
				};
				std::iter::once((index, "Rust", 1 + index % 7))
					.chain(late)
					.chain((index == 100).then_some((index, "Unknown", 50)))
			})
			.collect();
		let usage = usage(&lines, &[]);
		let every_week = timeline(&usage, 1, 1000);
		let sampled = timeline(&usage, 1, 150);
		assert_eq!(sampled.frames.len(), 150);
		for frame in &sampled.frames {
			let same_week = every_week
				.frames
				.iter()
				.find(|full| full.week == frame.week)
				.unwrap();
			assert_eq!(
				(frame.language_count, frame.total_lines),
				(same_week.language_count, same_week.total_lines)
			);
		}
		let last = sampled.frames.last().unwrap();
		assert_eq!(last.language_count, 3);
		let shown: u64 = last.entries.iter().map(|entry| entry.lines).sum();
		assert!(last.total_lines > shown);
	}

	#[test]
	fn resamples_to_max_frames_keeping_first_and_last() {
		let usage = usage(&[(0, "Rust", 1), (339, "Rust", 1)], &[]);
		let result = timeline(&usage, 10, 150);
		assert_eq!(result.frames.len(), 150);
		assert_eq!(result.frames[0].week, week(0));
		assert_eq!(result.frames[149].week, week(339));
		assert!(
			result
				.frames
				.windows(2)
				.all(|pair| pair[0].week < pair[1].week)
		);
	}

	#[test]
	fn tiny_max_frames_keeps_first_and_last() {
		let usage = usage(&[(0, "Rust", 1), (4, "Rust", 1)], &[]);
		for max_frames in [0, 1, 2] {
			let result = timeline(&usage, 10, max_frames);
			let weeks: Vec<_> = result.frames.iter().map(|frame| frame.week).collect();
			assert_eq!(weeks, [week(0), week(4)]);
		}
	}

	#[test]
	fn single_week_gives_single_frame() {
		let result = timeline(&usage(&[(3, "Rust", 1)], &[]), 10, 0);
		assert_eq!(result.frames.len(), 1);
		assert_eq!(result.frames[0].week, week(3));
	}

	#[test]
	fn fills_gap_weeks() {
		let usage = usage(&[(0, "Rust", 10), (3, "Rust", 5)], &[]);
		let result = timeline(&usage, 10, 100);
		assert_eq!(
			series_of(&result, "Rust"),
			[Some(10), Some(10), Some(10), Some(15)]
		);
		assert_eq!(result.frames[1].week, week(1));
	}

	#[test]
	fn leading_empty_weeks_are_skipped() {
		let mut usage = usage(&[(2, "Rust", 10)], &[]);
		usage.lines.insert(week(0), BTreeMap::new());
		usage.lines.insert(week(1), BTreeMap::from([("Rust", 0)]));
		let result = timeline(&usage, 10, 100);
		assert_eq!(result.frames[0].week, week(2));
	}

	#[test]
	fn language_without_lines_is_absent() {
		let usage = usage(&[(0, "Rust", 10), (0, "Go", 0)], &[]);
		assert_eq!(names(&timeline(&usage, 10, 100)), ["Rust"]);
	}

	#[test]
	fn empty_weeks_give_empty_timeline() {
		assert_eq!(timeline(&Usage::default(), 10, 100), Timeline::default());
		let zero = usage(&[(0, "Rust", 0), (1, "Rust", -3)], &[]);
		assert_eq!(timeline(&zero, 10, 100), Timeline::default());
	}

	#[test]
	fn output_is_deterministic() {
		assert_eq!(
			timeline(&five_languages(), 3, 2),
			timeline(&five_languages(), 3, 2)
		);
	}
}
