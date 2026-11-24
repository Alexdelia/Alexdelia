use std::cmp::Reverse;
use std::collections::BTreeSet;

use jiff::Span;

use crate::cache::Usage;
use crate::history::Week;
use crate::language::{Language, language_by_name};

const OTHER: Language = Language {
	name: "other",
	color: "#8b949e",
};
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
	pub repos: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
	pub week: Week,
	pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
	pub languages: Vec<LanguageSummary>,
	pub frames: Vec<Frame>,
}

struct Series {
	language: Language,
	weekly: Vec<u64>,
	first_use: Option<Vec<Week>>,
}

#[must_use]
pub fn timeline(usage: &Usage, top_n: usize, max_frames: usize) -> Timeline {
	let weeks = week_range(usage);
	if weeks.is_empty() {
		return Timeline::default();
	}
	let series_list = series_list(usage, &weeks, top_n);
	let cumulative_list: Vec<Vec<u64>> = series_list
		.iter()
		.map(|series| cumulative(&series.weekly))
		.collect();
	let languages = series_list
		.iter()
		.map(|series| summary(series, &weeks))
		.collect::<Vec<_>>();
	let frames = sample_indices(weeks.len(), max_frames)
		.into_iter()
		.map(|index| {
			let lines: Vec<u64> = cumulative_list.iter().map(|list| list[index]).collect();
			frame(weeks[index], &lines, &series_list, &languages)
		})
		.collect();
	Timeline { languages, frames }
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

fn series_list(usage: &Usage, weeks: &[Week], top_n: usize) -> Vec<Series> {
	let name_set: BTreeSet<&'static str> = usage
		.weeks
		.values()
		.flat_map(|lines| lines.keys().copied())
		.collect();
	let mut known = Vec::new();
	let mut folded = Vec::new();
	for name in name_set {
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
		if weekly.iter().sum::<u64>() == 0 {
			continue;
		}
		match language_by_name(name) {
			Some(language) => known.push(Series {
				language,
				weekly,
				first_use: Some(usage.first_use.get(name).cloned().unwrap_or_default()),
			}),
			None => folded.push(weekly),
		}
	}
	known.sort_by_key(|series| {
		(
			Reverse(series.weekly.iter().sum::<u64>()),
			series.language.name,
		)
	});
	folded.extend(
		known
			.split_off(top_n.min(known.len()))
			.into_iter()
			.map(|series| series.weekly),
	);
	if !folded.is_empty() {
		known.push(Series {
			language: OTHER,
			weekly: sum_elementwise(&folded, weeks.len()),
			first_use: None,
		});
	}
	known
}

fn sum_elementwise(list: &[Vec<u64>], length: usize) -> Vec<u64> {
	(0..length)
		.map(|index| list.iter().map(|weekly| weekly[index]).sum())
		.collect()
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
	series_list: &[Series],
	languages: &[LanguageSummary],
) -> Frame {
	let present: Vec<usize> = (0..lines.len()).filter(|index| lines[*index] > 0).collect();
	let present_lines: Vec<u64> = present.iter().map(|index| lines[*index]).collect();
	let mut entries: Vec<Entry> = present
		.iter()
		.zip(shares(&present_lines))
		.map(|(index, share_bp)| Entry {
			language: *index,
			lines: lines[*index],
			share_bp,
			repos: series_list[*index].first_use.as_ref().map(|first_use| {
				u32::try_from(first_use.partition_point(|first| *first <= week)).unwrap_or(u32::MAX)
			}),
		})
		.collect();
	entries.sort_by_key(|entry| {
		(
			Reverse(entry.share_bp),
			languages[entry.language].language.name,
		)
	});
	Frame { week, entries }
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
		assert_eq!(first, [("Rust", 3334), ("C", 3333), ("Go", 3333)]);
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
				.and_then(|entry| entry.repos)
		};
		let rust: Vec<_> = (0..4).map(|frame| repos(frame, "Rust")).collect();
		assert_eq!(rust, [Some(1), Some(1), Some(1), Some(2)]);
		assert_eq!(repos(1, "C"), Some(1));
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

	#[test]
	fn folds_beyond_top_n_into_other() {
		let result = timeline(&five_languages(), 3, 100);
		assert_eq!(names(&result), ["Rust", "C", "Go", "other"]);
		let other = summary_of(&result, "other");
		assert_eq!(other.language.color, "#8b949e");
		assert_eq!(other.since, week(1));
		assert_eq!(
			other.trend,
			Trend {
				last_year: 300,
				previous_year: 0
			}
		);
		let last = result.frames.last().unwrap();
		let entry = last
			.entries
			.iter()
			.find(|entry| result.languages[entry.language].language.name == "other")
			.unwrap();
		assert_eq!(entry.lines, 300);
		assert_eq!(entry.repos, None);
		let total: u32 = last.entries.iter().map(|entry| entry.share_bp).sum();
		assert_eq!(total, 10_000);
	}

	#[test]
	fn no_other_when_within_top_n() {
		let result = timeline(&five_languages(), 5, 100);
		assert_eq!(names(&result), ["Rust", "C", "Go", "Nix", "Python"]);
		let result = timeline(&five_languages(), 9, 100);
		assert_eq!(result.languages.len(), 5);
	}

	#[test]
	fn unknown_language_is_folded_into_other() {
		let usage = usage(&[(0, "Rust", 10), (0, "Unknown", 5)], &[]);
		let result = timeline(&usage, 10, 100);
		assert_eq!(names(&result), ["Rust", "other"]);
		assert_eq!(lines_of(&result, 0, "other"), Some(5));
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
