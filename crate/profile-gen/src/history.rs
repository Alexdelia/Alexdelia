use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use jiff::civil::Date;
use jiff::tz::Offset;
use jiff::{Span, Timestamp};

use crate::attributes::{Attribute, attributes};
use crate::error::Result;
use crate::git;
use crate::language::{Detector, Language};
use crate::ownership::{Owned, Ownership, ignored_revs, owned_lines_ignoring, ownership};

pub type Week = Date;
pub type LanguageDeltas = BTreeMap<&'static str, i64>;
pub type LanguageLines = BTreeMap<&'static str, u64>;
pub type Deltas = BTreeMap<Week, LanguageDeltas>;
pub type PathLines = BTreeMap<String, LanguageLines>;

pub(crate) const MAX_ADDED_PER_FILE: u64 = 5000;
const PATCH_SUFFIX: &str = ".patch";
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
	pub head: String,
	pub paths: PathLines,
	pub deltas: Deltas,
}

#[must_use]
pub fn week_of(unix_ts: i64) -> Week {
	let date = Offset::UTC
		.to_datetime(Timestamp::from_second(unix_ts).unwrap_or_default())
		.date();
	let since_monday = i64::from(date.weekday().to_monday_zero_offset());
	date.saturating_sub(Span::new().days(since_monday))
}

pub(crate) fn is_patch(path: &str) -> bool {
	path.to_lowercase().ends_with(PATCH_SUFFIX)
}

pub fn add_deltas(target: &mut Deltas, source: &Deltas) {
	for (week, bucket) in source {
		let sum = target.entry(*week).or_default();
		for (language, delta) in bucket {
			*sum.entry(language).or_default() += delta;
		}
		sum.retain(|_, delta| *delta != 0);
	}
	target.retain(|_, bucket| !bucket.is_empty());
}

pub fn walk(
	repo: &Path,
	since: Option<(&str, &PathLines)>,
	authors: &[String],
	detector: &Detector,
) -> Result<Option<Walk>> {
	git::head_sha(repo).and_then(|head| match (head, since) {
		(Some(head), Some((last_sha, paths))) if head == last_sha => Ok(Some(Walk {
			head,
			paths: paths.clone(),
			deltas: Deltas::new(),
		})),
		_ => walk_new_commits(repo, since, authors, detector),
	})
}

fn walk_new_commits(
	repo: &Path,
	since: Option<(&str, &PathLines)>,
	authors: &[String],
	detector: &Detector,
) -> Result<Option<Walk>> {
	ownership(repo, authors).and_then(|ownership| {
		let mut ignored_at: Option<(String, Vec<String>)> = None;
		walk_with(repo, since, &ownership, detector, |commit, path| {
			let ignored = match ignored_at.take() {
				Some((at, ignored)) if at == commit => Ok(ignored),
				_ => ignored_revs(repo, commit, &ownership),
			};
			ignored.and_then(|ignored| {
				let owned = owned_lines_ignoring(repo, commit, path, &ownership, &ignored);
				ignored_at = Some((commit.to_owned(), ignored));
				owned
			})
		})
	})
}

fn walk_with(
	repo: &Path,
	since: Option<(&str, &PathLines)>,
	ownership: &Ownership,
	detector: &Detector,
	blame: impl FnMut(&str, &str) -> Result<Owned>,
) -> Result<Option<Walk>> {
	git::head_sha(repo).and_then(|head| {
		head.map(|head| {
			snapshots(repo, since.map(|(sha, _)| sha)).and_then(|snapshot_list| {
				let start = Walker {
					repo,
					ownership,
					detector,
					blame,
					previous: since.map(|(sha, _)| sha.to_owned()),
					paths: since.map_or_default(|(_, paths)| paths.clone()),
					deltas: Deltas::new(),
				};
				snapshot_list
					.into_iter()
					.try_fold(start, |walker, (week, commit)| walker.advance(week, commit))
					.map(|walker| Walk {
						head,
						paths: walker.paths,
						deltas: walker.deltas,
					})
			})
		})
		.transpose()
	})
}

fn snapshots(repo: &Path, since: Option<&str>) -> Result<Vec<(Week, String)>> {
	let fresh = since.map(|sha| new_commits(repo, sha)).transpose();
	fresh.and_then(|fresh| {
		first_parent_weeks(repo).map(|chain| {
			let mut snapshot_list: Vec<(Week, String)> = Vec::new();
			let unseen = chain
				.into_iter()
				.filter(|(_, sha)| fresh.as_ref().is_none_or(|fresh| fresh.contains(sha)));
			for (week, sha) in unseen {
				if snapshot_list.last().is_some_and(|(last, _)| *last == week) {
					snapshot_list.pop();
				}
				snapshot_list.push((week, sha));
			}
			snapshot_list
		})
	})
}

fn new_commits(repo: &Path, since: &str) -> Result<HashSet<String>> {
	let range = format!("{since}..HEAD");
	git::run(repo, &["rev-list", "--first-parent", &range]).map(|listed| {
		String::from_utf8_lossy(&listed)
			.lines()
			.map(str::to_owned)
			.collect()
	})
}

fn first_parent_weeks(repo: &Path) -> Result<Vec<(Week, String)>> {
	let args = [
		"log",
		"--first-parent",
		"--reverse",
		"--format=%ct %H",
		"HEAD",
	];
	git::run(repo, &args).map(|log| {
		String::from_utf8_lossy(&log)
			.lines()
			.filter_map(|line| line.split_once(' '))
			.scan(Date::MIN, |latest, (timestamp, sha)| {
				*latest = (*latest).max(week_of(timestamp.parse().unwrap_or_default()));
				Some((*latest, sha.to_owned()))
			})
			.collect()
	})
}

#[derive(Debug)]
enum Change {
	Changed(String),
	Deleted(String),
	Moved {
		from: String,
		to: String,
		source_kept: bool,
	},
}

impl Change {
	fn source(&self) -> &str {
		match self {
			Self::Changed(path) | Self::Deleted(path) | Self::Moved { from: path, .. } => path,
		}
	}

	fn target(&self) -> Option<&str> {
		match self {
			Self::Changed(path) | Self::Moved { to: path, .. } => Some(path),
			Self::Deleted(_) => None,
		}
	}

	fn gone(&self) -> Option<&str> {
		match self {
			Self::Deleted(path)
			| Self::Moved {
				from: path,
				source_kept: false,
				..
			} => Some(path),
			Self::Changed(_)
			| Self::Moved {
				source_kept: true, ..
			} => None,
		}
	}

	fn candidate(&self, paths: &PathLines, touched: &HashSet<String>) -> Option<String> {
		let counted = match self {
			Self::Changed(path) => paths.contains_key(path) || touched.contains(path),
			Self::Moved { from, to, .. } => {
				paths.contains_key(from) || touched.contains(from) || touched.contains(to)
			}
			Self::Deleted(_) => false,
		};
		self.target().filter(|_| counted).map(str::to_owned)
	}
}

fn read_change<'a>(status: &str, tokens: &mut impl Iterator<Item = &'a str>) -> Change {
	let path = tokens.next().unwrap_or_default().to_owned();
	match status.as_bytes().first() {
		Some(b'D') => Change::Deleted(path),
		Some(kind @ (b'R' | b'C')) => Change::Moved {
			to: tokens.next().unwrap_or_default().to_owned(),
			from: path,
			source_kept: *kind == b'C',
		},
		_ => Change::Changed(path),
	}
}

fn changes(repo: &Path, previous: Option<&str>, commit: &str) -> Result<Vec<Change>> {
	let base = previous.unwrap_or(EMPTY_TREE);
	let args = [
		"diff-tree",
		"-r",
		"--name-status",
		"-z",
		"-M",
		"-C",
		base,
		commit,
	];
	git::run(repo, &args).map(|output| {
		let output = String::from_utf8_lossy(&output);
		let mut tokens = output.split('\0').filter(|token| !token.is_empty());
		std::iter::from_fn(|| tokens.next().map(|status| read_change(status, &mut tokens)))
			.collect()
	})
}

fn touched_by_me(
	repo: &Path,
	previous: Option<&str>,
	commit: &str,
	ownership: &Ownership,
	paths: &PathLines,
) -> Result<HashSet<String>> {
	let range = previous.map_or_else(|| commit.to_owned(), |base| format!("{base}..{commit}"));
	let args = [
		"log",
		"--reverse",
		"--no-merges",
		"--name-status",
		"-M",
		"-z",
		"--format=%x00%H",
		&range,
	];
	git::run(repo, &args)
		.map(|log| follow_touches(&String::from_utf8_lossy(&log), ownership, paths))
}

fn follow_touches(log: &str, ownership: &Ownership, paths: &PathLines) -> HashSet<String> {
	let mut touched = HashSet::new();
	for (sha, change_list) in commits(log) {
		let reaches_my_lines = change_list.iter().any(|change| {
			let source = change.source();
			paths.contains_key(source) || touched.contains(source)
		});
		if reaches_my_lines || ownership.is_mine(sha) {
			touched.extend(
				change_list
					.iter()
					.filter_map(Change::target)
					.map(str::to_owned),
			);
		}
	}
	touched
}

fn commits(log: &str) -> Vec<(&str, Vec<Change>)> {
	let mut commit_list: Vec<(&str, Vec<Change>)> = vec![("", Vec::new())];
	let mut tokens = log.split('\0');
	while let Some(token) = tokens.next() {
		if token.is_empty() {
			commit_list.push((tokens.next().unwrap_or_default(), Vec::new()));
		} else {
			let current = commit_list.len() - 1;
			commit_list[current]
				.1
				.push(read_change(token.trim_start_matches('\n'), &mut tokens));
		}
	}
	commit_list
}

#[derive(Debug, Clone, Copy)]
enum Counting {
	Plain(Language),
	Patch,
}

fn counting(path: &str, attribute: Attribute, detector: &Detector) -> Option<Counting> {
	match attribute {
		Attribute::Skip => None,
		_ if is_patch(path) => Some(Counting::Patch),
		Attribute::Language(name) => detector.count(name).map(Counting::Plain),
		Attribute::Default => detector.detect(path).map(Counting::Plain),
	}
}

fn signed(lines: u64) -> i64 {
	i64::try_from(lines).unwrap_or(i64::MAX)
}

struct Walker<'a, B> {
	repo: &'a Path,
	ownership: &'a Ownership,
	detector: &'a Detector,
	blame: B,
	previous: Option<String>,
	paths: PathLines,
	deltas: Deltas,
}

impl<B: FnMut(&str, &str) -> Result<Owned>> Walker<'_, B> {
	fn advance(mut self, week: Week, commit: String) -> Result<Self> {
		let previous = self.previous.as_deref();
		changes(self.repo, previous, &commit)
			.and_then(|change_list| {
				touched_by_me(self.repo, previous, &commit, self.ownership, &self.paths)
					.map(|touched| (change_list, touched))
			})
			.and_then(|(change_list, touched)| {
				let candidates: Vec<String> = change_list
					.iter()
					.filter_map(|change| change.candidate(&self.paths, &touched))
					.collect::<BTreeSet<_>>()
					.into_iter()
					.collect();
				let mut delta = LanguageDeltas::new();
				for gone in change_list.iter().filter_map(Change::gone) {
					let old = self.paths.remove(gone).unwrap_or_default();
					shift(&mut delta, &old, -1);
				}
				self.recount(&commit, &candidates, delta)
			})
			.map(|delta| {
				add_deltas(&mut self.deltas, &Deltas::from([(week, delta)]));
				self.previous = Some(commit);
				self
			})
	}

	fn recount(
		&mut self,
		commit: &str,
		candidates: &[String],
		delta: LanguageDeltas,
	) -> Result<LanguageDeltas> {
		attributes(self.repo, candidates).and_then(|attribute_map| {
			candidates
				.iter()
				.filter_map(|path| {
					counting(path, attribute_map[path], self.detector).map(|kind| (path, kind))
				})
				.try_fold(delta, |mut delta, (path, kind)| {
					(self.blame)(commit, path).map(|owned| {
						let new = language_lines(self.detector, kind, &owned);
						let old = self.paths.remove(path).unwrap_or_default();
						shift(&mut delta, &old, -1);
						shift(&mut delta, &new, 1);
						if !new.is_empty() {
							self.paths.insert(path.clone(), new);
						}
						delta
					})
				})
		})
	}
}

fn language_lines(detector: &Detector, kind: Counting, owned: &Owned) -> LanguageLines {
	let mut lines = LanguageLines::new();
	match kind {
		Counting::Plain(language) => {
			lines.insert(language.name, owned.plain);
		}
		Counting::Patch => {
			for (target, count) in &owned.patched {
				if let Some(language) = detector.detect(target) {
					*lines.entry(language.name).or_default() += count;
				}
			}
		}
	}
	lines.retain(|_, count| *count > 0);
	lines
}

fn shift(delta: &mut LanguageDeltas, lines: &LanguageLines, sign: i64) {
	for (language, count) in lines {
		*delta.entry(language).or_default() += sign * signed(*count);
	}
}

pub(crate) struct Numstat<'a> {
	pub(crate) added: Option<u64>,
	pub(crate) path: &'a str,
}

pub(crate) fn split_numstat(token: &str) -> Option<Numstat<'_>> {
	let mut fields = token.trim_start_matches('\n').splitn(3, '\t');
	match (fields.next(), fields.next(), fields.next()) {
		(Some(added), Some(_), Some(path)) => Some(Numstat {
			added: added.parse().ok(),
			path,
		}),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fixture::{FixtureRepo, shipped_detector};
	use crate::ownership::owned_lines;

	const WEEK_ONE_TS: i64 = 1_795_392_000;
	const DAY_SECONDS: i64 = 24 * 3600;
	const WEEK_SECONDS: i64 = 7 * DAY_SECONDS;
	const ME: &str = "me@x";
	const FRIEND: &str = "friend@x";

	fn code(tag: &str, range: std::ops::Range<usize>) -> String {
		range
			.map(|index| format!("int {tag}_value_{index}_alpha = compute_value({index}, 2, 3);\n"))
			.collect::<Vec<_>>()
			.concat()
	}

	fn at(week: i64) -> i64 {
		WEEK_ONE_TS + week * WEEK_SECONDS
	}

	fn week(offset: i64) -> Week {
		Date::constant(2026, 11, 23).saturating_add(Span::new().weeks(offset))
	}

	fn deltas(entries: &[(i64, &[(&'static str, i64)])]) -> Deltas {
		entries
			.iter()
			.map(|(offset, bucket)| (week(*offset), bucket.iter().copied().collect()))
			.collect()
	}

	fn lines(entries: &[(&str, &[(&'static str, u64)])]) -> PathLines {
		entries
			.iter()
			.map(|(path, bucket)| ((*path).to_owned(), bucket.iter().copied().collect()))
			.collect()
	}

	fn me() -> Vec<String> {
		vec![ME.to_owned()]
	}

	fn assert_levels_hold(walk: &Walk, since: Option<&PathLines>) {
		let mut levels: BTreeMap<&str, i64> = BTreeMap::new();
		for lines in since.into_iter().flat_map(BTreeMap::values) {
			for (language, count) in lines {
				*levels.entry(language).or_default() += signed(*count);
			}
		}
		for (week, bucket) in &walk.deltas {
			for (language, delta) in bucket {
				let level = levels.entry(language).or_default();
				*level += delta;
				assert!(*level >= 0, "{language} below zero at {week}: {walk:?}");
			}
		}
		let mut existing: BTreeMap<&str, i64> = BTreeMap::new();
		for lines in walk.paths.values() {
			for (language, count) in lines {
				*existing.entry(language).or_default() += signed(*count);
			}
		}
		levels.retain(|_, level| *level != 0);
		assert_eq!(levels, existing, "{walk:?}");
	}

	fn walked(repo: &Path, since: Option<(&str, &PathLines)>) -> Walk {
		let walk = walk(repo, since, &me(), &shipped_detector())
			.unwrap()
			.unwrap();
		assert_levels_hold(&walk, since.map(|(_, paths)| paths));
		walk
	}

	fn walked_recording(repo: &Path) -> (Walk, BTreeSet<String>) {
		let ownership = ownership(repo, &me()).unwrap();
		let mut blamed = BTreeSet::new();
		let walk = walk_with(
			repo,
			None,
			&ownership,
			&shipped_detector(),
			|commit, path| {
				blamed.insert(path.to_owned());
				owned_lines(repo, commit, path, &ownership)
			},
		)
		.unwrap()
		.unwrap();
		assert_levels_hold(&walk, None);
		(walk, blamed)
	}

	#[test]
	fn week_of_is_monday_utc() {
		assert_eq!(week_of(WEEK_ONE_TS), week(0));
		assert_eq!(week_of(WEEK_ONE_TS + WEEK_SECONDS - 60), week(0));
		assert_eq!(week_of(WEEK_ONE_TS + WEEK_SECONDS), week(1));
	}

	#[test]
	fn deleting_my_code_lowers_the_week() {
		let repo = FixtureRepo::new();
		repo.commit(
			ME,
			at(0),
			&[
				("a.c", Some(&code("a", 0..10))),
				("b.rs", Some(&code("b", 0..5))),
			],
		);
		repo.commit(FRIEND, at(0) + 1, &[("f.c", Some(&code("f", 0..7)))]);
		repo.commit(ME, at(1), &[("a.c", Some(&code("a", 0..6)))]);
		repo.commit(FRIEND, at(2), &[("b.rs", None)]);

		let walk = walked(repo.path(), None);

		assert_eq!(walk.head, repo.head());
		assert_eq!(
			walk.deltas,
			deltas(&[
				(0, &[("C", 10), ("Rust", 5)]),
				(1, &[("C", -4)]),
				(2, &[("Rust", -5)]),
			])
		);
		assert_eq!(walk.paths, lines(&[("a.c", &[("C", 6)])]));
	}

	#[test]
	fn rename_by_teammate_keeps_my_lines() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(0), &[("a.c", Some(&code("a", 0..6)))]);
		repo.mv("a.c", "dir/b.c", at(1));
		repo.commit(
			FRIEND,
			at(2),
			&[("dir/b.c", None), ("dir/c.h", Some(&code("a", 0..6)))],
		);

		let walk = walked(repo.path(), None);

		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 6)])]));
		assert_eq!(walk.paths, lines(&[("dir/c.h", &[("C", 6)])]));
	}

	#[test]
	fn rename_in_the_week_i_wrote_the_file_keeps_my_lines() {
		let repo = FixtureRepo::new();
		repo.commit(FRIEND, at(0), &[("seed.c", Some(&code("s", 0..2)))]);
		repo.commit(ME, at(1), &[("a.c", Some(&code("a", 0..6)))]);
		repo.mv("a.c", "b.c", at(1) + 1);

		let walk = walked(repo.path(), None);

		assert_eq!(walk.deltas, deltas(&[(1, &[("C", 6)])]));
		assert_eq!(walk.paths, lines(&[("b.c", &[("C", 6)])]));
	}

	#[test]
	fn moved_files_are_blamed_only_when_i_touched_them() {
		let repo = FixtureRepo::new();
		let friend_code = code("f", 0..6);
		let other_code = code("k", 0..6);
		repo.commit(
			FRIEND,
			at(0),
			&[("f.c", Some(&friend_code)), ("k.c", Some(&other_code))],
		);
		repo.commit(ME, at(0) + 1, &[("a.c", Some(&code("a", 0..3)))]);
		repo.mv("f.c", "g.c", at(1));
		repo.commit(ME, at(2), &[("g.c", None), ("h.c", Some(&friend_code))]);
		repo.commit(
			ME,
			at(3),
			&[("k.c", Some(&format!("{other_code}int mine_added = 1;\n")))],
		);
		repo.mv("k.c", "m.c", at(3) + 1);

		let (walk, blamed) = walked_recording(repo.path());

		assert_eq!(
			blamed,
			BTreeSet::from(["a.c".to_owned(), "h.c".to_owned(), "m.c".to_owned()])
		);
		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 3)]), (3, &[("C", 1)])]));
		assert_eq!(
			walk.paths,
			lines(&[("a.c", &[("C", 3)]), ("m.c", &[("C", 1)])])
		);
	}

	fn numbered(range: std::ops::Range<usize>) -> String {
		code("mine", range)
	}

	#[test]
	fn teammate_splits_my_file_into_new_files_keeps_my_lines() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(0), &[("a.c", Some(&numbered(0..30)))]);
		repo.commit(
			FRIEND,
			at(1),
			&[
				("a.c", Some(&numbered(0..10))),
				("b.c", Some(&numbered(10..20))),
				("c.c", Some(&numbered(20..30))),
			],
		);

		let walk = walked(repo.path(), None);

		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 30)])]));
		assert_eq!(
			walk.paths,
			lines(&[
				("a.c", &[("C", 10)]),
				("b.c", &[("C", 10)]),
				("c.c", &[("C", 10)]),
			])
		);
	}

	#[test]
	fn teammate_moves_my_lines_into_existing_file_keeps_them() {
		let repo = FixtureRepo::new();
		let theirs = code("f", 0..8);
		repo.commit(FRIEND, at(0), &[("f.c", Some(&theirs))]);
		repo.commit(ME, at(0) + 1, &[("a.c", Some(&numbered(0..20)))]);
		repo.commit(
			FRIEND,
			at(1),
			&[
				("a.c", Some(&numbered(0..10))),
				("f.c", Some(&format!("{theirs}{}", numbered(10..20)))),
			],
		);

		let walk = walked(repo.path(), None);

		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 20)])]));
		assert_eq!(
			walk.paths,
			lines(&[("a.c", &[("C", 10)]), ("f.c", &[("C", 10)])])
		);
	}

	#[test]
	fn heavy_rename_and_edit_in_one_week_keeps_my_lines() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(0), &[("a.c", Some(&numbered(0..6)))]);
		repo.mv("a.c", "b.c", at(1));
		repo.commit(
			FRIEND,
			at(1) + 1,
			&[(
				"b.c",
				Some(&format!("{}{}", numbered(0..6), code("f", 0..30))),
			)],
		);

		let walk = walked(repo.path(), None);

		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 6)])]));
		assert_eq!(walk.paths, lines(&[("b.c", &[("C", 6)])]));
	}

	#[test]
	fn teammate_edits_untouched_file_cost_no_blame() {
		let repo = FixtureRepo::new();
		repo.commit(FRIEND, at(0), &[("f.c", Some(&code("f", 0..5)))]);
		repo.commit(ME, at(0) + 1, &[("a.c", Some(&code("a", 0..3)))]);
		repo.commit(FRIEND, at(1), &[("f.c", Some(&code("f", 0..9)))]);
		repo.commit(FRIEND, at(2), &[("g.c", Some(&code("f", 0..4)))]);

		let (walk, blamed) = walked_recording(repo.path());

		assert_eq!(blamed, BTreeSet::from(["a.c".to_owned()]));
		assert!(!walk.paths.contains_key("f.c"));
		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 3)])]));
	}

	#[test]
	fn weeks_never_go_back_with_skewed_dates() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(2), &[("a.c", Some(&code("a", 0..3)))]);
		repo.commit(ME, at(0), &[("b.c", Some(&code("b", 0..2)))]);
		repo.commit(ME, at(3), &[("c.rs", Some(&code("c", 0..1)))]);
		repo.commit(ME, at(1), &[("c.rs", None)]);
		repo.commit(ME, at(4), &[("d.rs", Some(&code("d", 0..4)))]);

		let walk = walked(repo.path(), None);

		assert_eq!(
			walk.deltas,
			deltas(&[(2, &[("C", 5)]), (4, &[("Rust", 4)])])
		);
	}

	#[test]
	fn incremental_walk_from_mid_week_equals_full_walk() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(0), &[("a.c", Some(&code("a", 0..5)))]);
		repo.commit(
			ME,
			at(0) + DAY_SECONDS,
			&[
				("a.c", Some(&code("a", 0..8))),
				("b.rs", Some(&code("b", 0..2))),
			],
		);
		let first = walked(repo.path(), None);
		repo.commit(ME, at(0) + 2 * DAY_SECONDS, &[("b.rs", None)]);
		repo.commit(
			FRIEND,
			at(0) + 3 * DAY_SECONDS,
			&[("f.c", Some(&code("f", 0..3)))],
		);
		repo.mv("a.c", "c.c", at(1));
		repo.commit(ME, at(2), &[("c.c", Some(&code("a", 0..5)))]);
		let bare = repo.bare_clone();

		let second = walked(bare.path(), Some((&first.head, &first.paths)));
		let full = walked(bare.path(), None);

		let mut combined = first.deltas;
		add_deltas(&mut combined, &second.deltas);
		assert_eq!(combined, full.deltas);
		assert_eq!(
			second,
			Walk {
				deltas: second.deltas.clone(),
				..full.clone()
			}
		);
		assert_eq!(full.deltas, deltas(&[(0, &[("C", 8)]), (2, &[("C", -3)])]));
		assert_eq!(full.paths, lines(&[("c.c", &[("C", 5)])]));
	}

	#[test]
	fn empty_commits_change_nothing() {
		let repo = FixtureRepo::new();
		repo.empty_commit(ME, at(0));
		repo.commit(ME, at(0) + 1, &[("a.c", Some(&code("a", 0..2)))]);
		repo.empty_commit(ME, at(0) + 2);
		repo.empty_commit(FRIEND, at(1));
		repo.commit(ME, at(1) + 1, &[("a.c", Some(&code("a", 0..3)))]);
		repo.empty_commit(ME, at(2));

		let walk = walked(repo.path(), None);

		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 2)]), (1, &[("C", 1)])]));
	}

	#[test]
	fn since_head_gives_nothing() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(0), &[("a.c", Some(&code("a", 0..2)))]);
		let first = walked(repo.path(), None);

		let again = walked(repo.path(), Some((&first.head, &first.paths)));

		assert!(again.deltas.is_empty());
		assert_eq!(again.paths, first.paths);
		assert_eq!(again.head, first.head);
	}

	#[test]
	fn vendored_and_unknown_files_never_blamed() {
		let repo = FixtureRepo::new();
		repo.commit(
			ME,
			at(0),
			&[
				(
					".gitattributes",
					Some(
						"vendor/** linguist-vendored\n*.cfgx linguist-language=json\n*.txtx linguist-language=Text\n",
					),
				),
				("vendor/v.c", Some(&code("v", 0..4))),
				("notes.unknownext", Some(&code("n", 0..4))),
				("logo.png", Some("\0\0\0png")),
				("Cargo.lock", Some(&code("l", 0..4))),
				("a.txtx", Some(&code("t", 0..4))),
				("a.cfgx", Some(&code("j", 0..2))),
				("huge.c", Some(&code("h", 0..5001))),
				("a.c", Some(&code("a", 0..3))),
			],
		);
		repo.commit(ME, at(1), &[("vendor/v.c", Some(&code("v", 0..9)))]);

		let (walk, blamed) = walked_recording(repo.path());

		assert_eq!(
			blamed,
			BTreeSet::from(["a.c".to_owned(), "a.cfgx".to_owned(), "huge.c".to_owned()])
		);
		assert_eq!(walk.deltas, deltas(&[(0, &[("C", 3), ("JSON", 2)])]));
		assert_eq!(
			walk.paths,
			lines(&[("a.c", &[("C", 3)]), ("a.cfgx", &[("JSON", 2)])])
		);
	}

	const PATCH: &str = "From 0 Mon Sep 17 00:00:00 2001\nSubject: [PATCH] fix\n\n---\ndiff --git a/src/a.c b/src/a.c\nindex 1..2 100644\n--- a/src/a.c\n+++ b/src/a.c\n@@ -1,2 +1,4 @@\n int a;\n-int b;\n+int c;\n+int d;\n+int e;\ndiff --git a/lib.rs b/lib.rs\n--- a/lib.rs\n+++ b/lib.rs\t2026-01-01\n@@ -1 +1,2 @@\n+fn a() {}\n+fn b() {}\n fn c() {}\n";

	#[test]
	fn patch_lines_count_for_the_patched_language() {
		let repo = FixtureRepo::new();
		repo.commit(
			ME,
			at(0),
			&[
				(
					".gitattributes",
					Some("vendor/** linguist-vendored\nforced.patch linguist-language=Python\n"),
				),
				("patches/fix.patch", Some(PATCH)),
				("vendor/x.patch", Some(PATCH)),
				("forced.patch", Some(&PATCH.replace("lib.rs", "lib.py"))),
				(
					"docs.patch",
					Some(&PATCH.replace("lib.rs", "notes.unknownext")),
				),
			],
		);
		repo.commit(FRIEND, at(0) + 1, &[("other.patch", Some(PATCH))]);
		repo.commit(ME, at(1), &[("forced.patch", None)]);

		let walk = walked(repo.path(), None);

		assert_eq!(
			walk.deltas,
			deltas(&[
				(0, &[("C", 9), ("Python", 2), ("Rust", 2)]),
				(1, &[("C", -3), ("Python", -2)]),
			])
		);
		assert_eq!(
			walk.paths,
			lines(&[
				("docs.patch", &[("C", 3)]),
				("patches/fix.patch", &[("C", 3), ("Rust", 2)]),
			])
		);
	}

	#[test]
	fn empty_repo_walks_to_none() {
		let repo = FixtureRepo::new();
		let bare = repo.bare_clone();
		for path in [repo.path(), bare.path()] {
			assert_eq!(walk(path, None, &me(), &shipped_detector()).unwrap(), None);
		}
	}

	#[test]
	fn fails_outside_a_repository() {
		let dir = tempfile::TempDir::new().unwrap();
		assert!(walk(dir.path(), None, &me(), &shipped_detector()).is_err());
	}

	#[test]
	fn unknown_since_is_an_error() {
		let repo = FixtureRepo::new();
		repo.commit(ME, at(0), &[("a.c", Some(&code("a", 0..2)))]);
		let message = walk(
			repo.path(),
			Some(("deadbeef", &PathLines::new())),
			&me(),
			&shipped_detector(),
		)
		.unwrap_err()
		.to_string();
		assert!(message.starts_with("git rev-list in "), "{message}");
	}

	#[test]
	fn add_deltas_drops_what_cancels_out() {
		let mut target = deltas(&[(0, &[("C", 3), ("Rust", 1)]), (1, &[("C", 2)])]);
		add_deltas(
			&mut target,
			&deltas(&[(0, &[("Rust", -1)]), (1, &[("C", -2)]), (2, &[("Nix", 4)])]),
		);
		assert_eq!(target, deltas(&[(0, &[("C", 3)]), (2, &[("Nix", 4)])]));
	}
}
