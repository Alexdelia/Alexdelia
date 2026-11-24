use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use jiff::Span;

use crate::cache::Usage;
use crate::history::Week;
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
	pub last_year: u64,
	pub previous_year: u64,
}

impl Trend {
	#[must_use]
	pub const fn direction(&self) -> Direction {
		if self.last_year * 10 > self.previous_year * 11
			|| (self.previous_year == 0 && self.last_year > 0)
		{
			Direction::Up
		} else if self.last_year * 10 < self.previous_year * 9 {
			Direction::Down
		} else {
			Direction::Flat
		}
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

struct Totals {
	language_count: u32,
	total_lines: u64,
}

struct Series {
	language: Language,
	weekly: Vec<u64>,
	first_use: Vec<Week>,
}

#[must_use]
pub fn timeline(usage: &Usage, top_n: usize, max_frames: usize) -> Timeline {
	let weeks = week_range(usage);
	if weeks.is_empty() {
		return Timeline::default();
	}
	let frame_indices = sample_indices(weeks.len(), max_frames);
	let known = known_series(usage, &weeks);
	let known_cumulative: Vec<Vec<u64>> = known
		.iter()
		.map(|series| cumulative(&series.weekly))
		.collect();
	let shown = within_top_n_somewhere(&known, &known_cumulative, &frame_indices, top_n);
	let all_cumulative = cumulative(&all_weekly(usage, &weeks));
	let first_code_list = first_code_indices(usage, &weeks);
	let series_list: Vec<&Series> = shown.iter().map(|index| &known[*index]).collect();
	let languages: Vec<LanguageSummary> = series_list
		.iter()
		.map(|series| summary(series, &weeks))
		.collect();
	let frames = frame_indices
		.into_iter()
		.map(|index| {
			let lines: Vec<u64> = shown
				.iter()
				.map(|known_index| known_cumulative[*known_index][index])
				.collect();
			let totals = Totals {
				language_count: u32::try_from(
					first_code_list.partition_point(|first| *first <= index),
				)
				.unwrap_or(u32::MAX),
				total_lines: all_cumulative[index],
			};
			frame(weeks[index], &lines, &totals, &series_list, &languages)
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
		.weeks
		.iter()
		.find(|(_, lines)| lines.values().any(|count| *count > 0))
		.map(|(week, _)| *week);
	let Some(first) = first else {
		return Vec::new();
	};
	let last = usage.weeks.keys().next_back().copied().unwrap_or(first);
	std::iter::successors(Some(first), |week| {
		Some(week.saturating_add(Span::new().weeks(1)))
	})
	.take_while(|week| *week <= last)
	.collect()
}

fn known_series(usage: &Usage, weeks: &[Week]) -> Vec<Series> {
	let name_set: BTreeSet<&'static str> = usage
		.weeks
		.values()
		.flat_map(|lines| lines.keys().copied())
		.collect();
	let mut known: Vec<Series> = name_set
		.into_iter()
		.filter_map(|name| {
			let language = language_by_name(name)?;
			let weekly: Vec<u64> = weeks
				.iter()
				.map(|week| {
					usage
						.weeks
						.get(week)
						.and_then(|lines| lines.get(name))
						.copied()
						.unwrap_or(0)
				})
				.collect();
			(weekly.iter().sum::<u64>() > 0).then(|| Series {
				language,
				weekly,
				first_use: usage.first_use.get(name).cloned().unwrap_or_default(),
			})
		})
		.collect();
	known.sort_by_key(|series| {
		(
			Reverse(series.weekly.iter().sum::<u64>()),
			series.language.name,
		)
	});
	known
}

fn all_weekly(usage: &Usage, weeks: &[Week]) -> Vec<u64> {
	weeks
		.iter()
		.map(|week| {
			usage
				.weeks
				.get(week)
				.map_or(0, |lines| lines.values().sum())
		})
		.collect()
}

fn first_code_indices(usage: &Usage, weeks: &[Week]) -> Vec<usize> {
	let mut first_by_name: BTreeMap<&'static str, usize> = BTreeMap::new();
	for (index, week) in weeks.iter().enumerate() {
		let added = usage.weeks.get(week).into_iter().flatten();
		for (name, _) in added.filter(|(_, lines)| **lines > 0) {
			first_by_name.entry(name).or_insert(index);
		}
	}
	let mut first_list: Vec<usize> = first_by_name.into_values().collect();
	first_list.sort_unstable();
	first_list
}

fn within_top_n_somewhere(
	known: &[Series],
	known_cumulative: &[Vec<u64>],
	frame_indices: &[usize],
	top_n: usize,
) -> Vec<usize> {
	let mut shown = vec![false; known.len()];
	for index in frame_indices {
		let mut ranked: Vec<usize> = (0..known.len())
			.filter(|known_index| known_cumulative[*known_index][*index] > 0)
			.collect();
		ranked.sort_by_key(|known_index| {
			(
				Reverse(known_cumulative[*known_index][*index]),
				known[*known_index].language.name,
			)
		});
		for known_index in ranked.into_iter().take(top_n) {
			shown[known_index] = true;
		}
	}
	(0..known.len()).filter(|index| shown[*index]).collect()
}

fn cumulative(weekly: &[u64]) -> Vec<u64> {
	weekly
		.iter()
		.scan(0, |total, added| {
			*total += added;
			Some(*total)
		})
		.collect()
}

fn summary(series: &Series, weeks: &[Week]) -> LanguageSummary {
	let weekly = &series.weekly;
	let since = weekly
		.iter()
		.position(|added| *added > 0)
		.map_or(weeks[0], |index| weeks[index]);
	let count = weekly.len();
	let last_start = count.saturating_sub(TREND_WEEKS);
	let previous_start = count.saturating_sub(2 * TREND_WEEKS);
	LanguageSummary {
		language: series.language,
		since,
		trend: Trend {
			last_year: weekly[last_start..].iter().sum(),
			previous_year: weekly[previous_start..last_start].iter().sum(),
		},
	}
}

fn sample_indices(count: usize, max_frames: usize) -> Vec<usize> {
	let target = max_frames.max(2);
	if count <= target {
		return (0..count).collect();
	}
	(0..target)
		.map(|step| step * (count - 1) / (target - 1))
		.collect()
}

fn frame(
	week: Week,
	lines: &[u64],
	totals: &Totals,
	series_list: &[&Series],
	languages: &[LanguageSummary],
) -> Frame {
	let present: Vec<usize> = (0..lines.len()).filter(|index| lines[*index] > 0).collect();
	let mut counted: Vec<u64> = present.iter().map(|index| lines[*index]).collect();
	counted.push(totals.total_lines - lines.iter().sum::<u64>());
	let mut entries: Vec<Entry> = present
		.iter()
		.zip(shares(&counted))
		.map(|(index, share_bp)| Entry {
			language: *index,
			lines: lines[*index],
			share_bp,
			repos: u32::try_from(
				series_list[*index]
					.first_use
					.partition_point(|first| *first <= week),
			)
			.unwrap_or(u32::MAX),
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
	let total = u128::from(lines.iter().sum::<u64>());
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

	fn usage(added: &[(i64, &'static str, u64)], first: &[(&'static str, &[i64])]) -> Usage {
		let mut usage = Usage::default();
		for (index, language, lines) in added {
			*usage
				.weeks
				.entry(week(*index))
				.or_default()
				.entry(language)
				.or_default() += lines;
		}
		for (language, indices) in first {
			usage
				.first_use
				.insert(language, indices.iter().map(|index| week(*index)).collect());
		}
		usage
	}

	fn names(timeline: &Timeline) -> Vec<&'static str> {
		timeline
			.languages
			.iter()
			.map(|summary| summary.language.name)
			.collect()
	}

	fn lines_of(timeline: &Timeline, frame: usize, name: &str) -> Option<u64> {
		timeline.frames[frame]
			.entries
			.iter()
			.find(|entry| timeline.languages[entry.language].language.name == name)
			.map(|entry| entry.lines)
	}

	fn sample() -> Usage {
		usage(
			&[
				(0, "Rust", 100),
				(1, "Rust", 50),
				(1, "C", 10),
				(2, "C", 300),
				(3, "Rust", 5),
			],
			&[("Rust", &[0, 3]), ("C", &[1])],
		)
	}

	#[test]
	fn cumulative_is_monotonic_and_final_matches_totals() {
		let result = timeline(&sample(), 10, 100);
		assert_eq!(result.frames.len(), 4);
		let rust: Vec<_> = (0..4)
			.map(|frame| lines_of(&result, frame, "Rust"))
			.collect();
		assert_eq!(rust, [Some(100), Some(150), Some(150), Some(155)]);
		let c: Vec<_> = (0..4).map(|frame| lines_of(&result, frame, "C")).collect();
		assert_eq!(c, [None, Some(10), Some(310), Some(310)]);
		assert_eq!(names(&result), ["C", "Rust"]);
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
		let order = |frame: usize| -> Vec<&str> {
			result.frames[frame]
				.entries
				.iter()
				.map(|entry| result.languages[entry.language].language.name)
				.collect()
		};
		assert_eq!(order(0), ["Rust"]);
		assert_eq!(order(1), ["Rust", "C"]);
		assert_eq!(order(2), ["C", "Rust"]);
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
	fn repo_count_follows_first_use() {
		let result = timeline(&sample(), 10, 100);
		let repos = |frame: usize, name: &str| {
			result.frames[frame]
				.entries
				.iter()
				.find(|entry| result.languages[entry.language].language.name == name)
				.map(|entry| entry.repos)
		};
		let rust: Vec<_> = (0..4).map(|frame| repos(frame, "Rust")).collect();
		assert_eq!(rust, [Some(1), Some(1), Some(1), Some(2)]);
		assert_eq!(repos(1, "C"), Some(1));
		let unlisted = timeline(&usage(&[(0, "Go", 5)], &[]), 10, 100);
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
	fn since_and_trend() {
		let added = [
			(0, "Rust", 1),
			(60, "Rust", 100),
			(120, "Rust", 200),
			(160, "Rust", 1),
			(0, "Go", 10),
			(60, "Go", 1000),
			(150, "Go", 100),
			(0, "Python", 1),
			(60, "Python", 100),
			(150, "Python", 105),
			(150, "Nix", 5),
		];
		let result = timeline(&usage(&added, &[]), 10, 1000);
		assert_eq!(result.frames.len(), 161);

		let rust = summary_of(&result, "Rust");
		assert_eq!(rust.since, week(0));
		assert_eq!(
			rust.trend,
			Trend {
				last_year: 201,
				previous_year: 100
			}
		);
		assert_eq!(rust.trend.direction(), Direction::Up);
		assert_eq!(summary_of(&result, "Go").trend.direction(), Direction::Down);
		assert_eq!(
			summary_of(&result, "Python").trend.direction(),
			Direction::Flat
		);
		let nix = summary_of(&result, "Nix");
		assert_eq!(nix.since, week(150));
		assert_eq!(nix.trend.previous_year, 0);
		assert_eq!(nix.trend.direction(), Direction::Up);
	}

	#[test]
	fn direction_thresholds() {
		let direction = |last_year, previous_year| {
			Trend {
				last_year,
				previous_year,
			}
			.direction()
		};
		assert_eq!(direction(110, 100), Direction::Flat);
		assert_eq!(direction(111, 100), Direction::Up);
		assert_eq!(direction(90, 100), Direction::Flat);
		assert_eq!(direction(89, 100), Direction::Down);
		assert_eq!(direction(0, 0), Direction::Flat);
		assert_eq!(direction(1, 0), Direction::Up);
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
			&[("Rust", &[0]), ("Nix", &[2]), ("Python", &[1])],
		)
	}

	fn share_of(timeline: &Timeline, frame: usize, name: &str) -> Option<u32> {
		timeline.frames[frame]
			.entries
			.iter()
			.find(|entry| timeline.languages[entry.language].language.name == name)
			.map(|entry| entry.share_bp)
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
		let order = |frame: usize| -> Vec<&str> {
			result.frames[frame]
				.entries
				.iter()
				.map(|entry| result.languages[entry.language].language.name)
				.collect()
		};
		assert_eq!(order(0), ["Go", "Rust"]);
		assert_eq!(order(1), ["C", "Go", "Rust"]);
		assert_eq!(order(2), ["Python", "C", "Go", "Rust"]);
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

	#[test]
	fn totals_count_every_language_with_code_including_hidden() {
		let result = timeline(&five_languages(), 3, 100);
		assert_eq!(names(&result).len(), 3);
		assert_eq!(totals(&result), [(3, 1200), (4, 1300), (5, 1500)]);
		let unknown = usage(&[(0, "Rust", 10), (1, "Unknown", 100)], &[]);
		assert_eq!(totals(&timeline(&unknown, 1, 100)), [(1, 10), (2, 110)]);
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
		let added: Vec<(i64, &'static str, u64)> = (0..340)
			.flat_map(|index| {
				let late = if index >= 200 {
					vec![(index, "Go", 3)]
				} else {
					Vec::new()
				};
				std::iter::once((index, "Rust", 1 + u64::try_from(index % 7).unwrap()))
					.chain(late)
					.chain((index == 100).then_some((index, "Unknown", 50)))
			})
			.collect();
		let usage = usage(&added, &[]);
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
		let rust: Vec<_> = (0..4)
			.map(|frame| lines_of(&result, frame, "Rust"))
			.collect();
		assert_eq!(rust, [Some(10), Some(10), Some(10), Some(15)]);
		assert_eq!(result.frames[1].week, week(1));
	}

	#[test]
	fn leading_empty_weeks_are_skipped() {
		let mut usage = usage(&[(2, "Rust", 10)], &[]);
		usage.weeks.insert(week(0), BTreeMap::new());
		usage.weeks.insert(week(1), BTreeMap::from([("Rust", 0)]));
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
		let zero = usage(&[(0, "Rust", 0)], &[]);
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
