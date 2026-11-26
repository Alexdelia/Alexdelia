use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdout, Output, Stdio};

use jiff::civil::Date;
use jiff::tz::Offset;
use jiff::{Span, Timestamp};

use crate::attributes::{Attribute, attributes};
use crate::error::Result;
use crate::git;
use crate::language::{Detector, Language};

pub type Week = Date;
pub type Weeks = BTreeMap<Week, BTreeMap<&'static str, u64>>;

const MAX_ADDED_PER_FILE: u64 = 5000;
const PATCH_SUFFIX: &str = ".patch";
const PATCH_PATHSPEC: &str = ":(icase)*.patch";
const WHOLE_FILE_CONTEXT: &str = "--unified=1000000";

#[derive(Debug)]
struct Touch {
	week: Week,
	path: String,
	added: u64,
	patched: Option<String>,
}

#[must_use]
pub fn week_of(unix_ts: i64) -> Week {
	let date = Offset::UTC
		.to_datetime(Timestamp::from_second(unix_ts).unwrap_or_default())
		.date();
	let since_monday = i64::from(date.weekday().to_monday_zero_offset());
	date.saturating_sub(Span::new().days(since_monday))
}

pub fn weekly_added(
	repo: &Path,
	since: Option<&str>,
	authors: &[String],
	detector: &Detector,
) -> Result<(Weeks, Option<String>)> {
	let Some(head) = git::head_sha(repo)? else {
		return Ok((Weeks::new(), None));
	};
	let wanted: HashSet<String> = authors.iter().map(|author| author.to_lowercase()).collect();
	let range = since.map_or_else(|| "HEAD".to_owned(), |sha| format!("{sha}..HEAD"));
	read_touches(repo, &range, &wanted)
		.and_then(|touches| with_patched_lines(repo, &range, &wanted, touches))
		.and_then(|touches| {
			let paths: Vec<String> = touches
				.iter()
				.map(|touch| touch.path.clone())
				.collect::<HashSet<_>>()
				.into_iter()
				.collect();
			attributes(repo, &paths).map(|attribute_map| tally(&touches, &attribute_map, detector))
		})
		.map(|weeks| (weeks, Some(head)))
}

fn is_patch(path: &str) -> bool {
	path.to_lowercase().ends_with(PATCH_SUFFIX)
}

fn with_patched_lines(
	repo: &Path,
	range: &str,
	wanted: &HashSet<String>,
	touches: Vec<Touch>,
) -> Result<Vec<Touch>> {
	let (patches, plain): (Vec<Touch>, Vec<Touch>) =
		touches.into_iter().partition(|touch| is_patch(&touch.path));
	if patches.is_empty() {
		return Ok(plain);
	}
	read_patch_touches(repo, range, wanted)
		.map(|patched| plain.into_iter().chain(patched).collect())
}

fn tally(
	touches: &[Touch],
	attribute_map: &std::collections::HashMap<String, Attribute>,
	detector: &Detector,
) -> Weeks {
	let mut weeks = Weeks::new();
	for touch in touches {
		if let Some(language) = resolve_language(touch, attribute_map[&touch.path], detector) {
			*weeks
				.entry(touch.week)
				.or_default()
				.entry(language.name)
				.or_default() += touch.added;
		}
	}
	weeks
}

fn resolve_language(touch: &Touch, attribute: Attribute, detector: &Detector) -> Option<Language> {
	match (attribute, &touch.patched) {
		(Attribute::Skip, _) => None,
		(_, Some(target)) => detector.detect(target),
		(Attribute::Language(name), None) => detector.count(name),
		(Attribute::Default, None) => detector.detect(&touch.path),
	}
}

fn read_touches(repo: &Path, range: &str, wanted: &HashSet<String>) -> Result<Vec<Touch>> {
	let args = [
		"log",
		"--no-merges",
		"-M",
		"--numstat",
		"-z",
		"--format=%H%x00%ae%x00%at",
		range,
	];
	run_log(repo, &args, |reader| parse_touches(reader, wanted))
}

fn read_patch_touches(repo: &Path, range: &str, wanted: &HashSet<String>) -> Result<Vec<Touch>> {
	let args = [
		"log",
		"--no-merges",
		"-M",
		"-p",
		"--no-prefix",
		WHOLE_FILE_CONTEXT,
		"--format=%x00%ae%x00%at",
		range,
		"--",
		PATCH_PATHSPEC,
	];
	run_log(repo, &args, |reader| parse_patch_touches(reader, wanted))
}

fn run_log(
	repo: &Path,
	args: &[&str],
	parse: impl FnOnce(BufReader<ChildStdout>) -> std::io::Result<Vec<Touch>>,
) -> Result<Vec<Touch>> {
	git::command(repo)
		.args(args)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.and_then(|child| collect(child, parse))
		.map_err(Into::into)
		.and_then(|(touches, output)| {
			if output.status.success() {
				Ok(touches)
			} else {
				Err(git::failure(repo, args, &output.stderr))
			}
		})
}

fn collect(
	mut child: Child,
	parse: impl FnOnce(BufReader<ChildStdout>) -> std::io::Result<Vec<Touch>>,
) -> std::io::Result<(Vec<Touch>, Output)> {
	let touches = child
		.stdout
		.take()
		.map(|stdout| parse(BufReader::new(stdout)))
		.transpose()
		.map(Option::unwrap_or_default);
	let exit = child.wait_with_output();
	touches.and_then(|touches| exit.map(|output| (touches, output)))
}

fn read_token(reader: &mut impl BufRead, delimiter: u8) -> std::io::Result<Option<String>> {
	let mut bytes = Vec::new();
	reader.read_until(delimiter, &mut bytes).map(|read| {
		(read > 0).then(|| {
			if bytes.last() == Some(&delimiter) {
				bytes.pop();
			}
			String::from_utf8_lossy(&bytes).into_owned()
		})
	})
}

fn feed_tokens(
	mut reader: impl BufRead,
	delimiter: u8,
	mut feed: impl FnMut(&str),
) -> std::io::Result<()> {
	let mut failure = None;
	std::iter::from_fn(|| match read_token(&mut reader, delimiter) {
		Ok(token) => token,
		Err(error) => {
			failure = Some(std::io::Error::new(
				error.kind(),
				format!("reading git log output: {error}"),
			));
			None
		}
	})
	.for_each(|token| feed(&token));
	failure.map_or(Ok(()), Err)
}

fn commit_week(timestamp: &str, counted: bool) -> Option<Week> {
	timestamp.parse().ok().map(week_of).filter(|_| counted)
}

#[derive(Debug)]
enum Phase {
	Sha,
	Email,
	Timestamp {
		counted: bool,
	},
	Entries {
		week: Option<Week>,
	},
	RenamedFrom {
		week: Option<Week>,
		added: Option<u64>,
	},
	RenamedTo {
		week: Option<Week>,
		added: Option<u64>,
	},
}

struct Parser<'a> {
	wanted: &'a HashSet<String>,
	phase: Phase,
	touches: Vec<Touch>,
}

impl Parser<'_> {
	fn feed(&mut self, token: &str) {
		self.phase = match std::mem::replace(&mut self.phase, Phase::Sha) {
			Phase::Sha => Phase::Email,
			Phase::Email => Phase::Timestamp {
				counted: self.wanted.contains(&token.to_lowercase()),
			},
			Phase::Timestamp { counted } => Phase::Entries {
				week: commit_week(token, counted),
			},
			Phase::Entries { week } => self.feed_entry(token, week),
			Phase::RenamedFrom { week, added } => Phase::RenamedTo { week, added },
			Phase::RenamedTo { week, added } => {
				self.record(week, added, token);
				Phase::Entries { week }
			}
		};
	}

	fn feed_entry(&mut self, token: &str, week: Option<Week>) -> Phase {
		match split_numstat(token) {
			None => Phase::Email,
			Some(entry) if entry.path.is_empty() => Phase::RenamedFrom {
				week,
				added: entry.added,
			},
			Some(entry) => {
				self.record(week, entry.added, entry.path);
				Phase::Entries { week }
			}
		}
	}

	fn record(&mut self, week: Option<Week>, added: Option<u64>, path: &str) {
		let within_cap = added.filter(|added| (1..=MAX_ADDED_PER_FILE).contains(added));
		if let (Some(week), Some(added)) = (week, within_cap) {
			self.touches.push(Touch {
				week,
				path: path.to_owned(),
				added,
				patched: None,
			});
		}
	}
}

fn parse_touches(reader: impl BufRead, wanted: &HashSet<String>) -> std::io::Result<Vec<Touch>> {
	let mut parser = Parser {
		wanted,
		phase: Phase::Sha,
		touches: Vec::new(),
	};
	feed_tokens(reader, 0, |token| parser.feed(token)).map(|()| parser.touches)
}

struct PatchParser<'a> {
	wanted: &'a HashSet<String>,
	week: Option<Week>,
	file: Option<String>,
	in_hunk: bool,
	after_old_header: bool,
	target: Option<String>,
	added: u64,
	target_lines: BTreeMap<String, u64>,
	touches: Vec<Touch>,
}

impl PatchParser<'_> {
	fn feed(&mut self, line: &str) {
		let line = line.trim_end_matches('\r');
		if let Some(header) = line.strip_prefix('\0') {
			self.flush();
			self.week = header.split_once('\0').and_then(|(email, timestamp)| {
				commit_week(timestamp, self.wanted.contains(&email.to_lowercase()))
			});
		} else if line.starts_with("diff --git ") {
			self.flush();
		} else if self.in_hunk {
			self.feed_hunk(line);
		} else {
			self.feed_header(line);
		}
	}

	fn feed_header(&mut self, line: &str) {
		if let Some(path) = line.strip_prefix("+++ ") {
			self.file = Some(git::unquote_path(path));
		}
		self.in_hunk = line.starts_with("@@ ");
	}

	fn feed_hunk(&mut self, line: &str) {
		if let Some(content) = line.strip_prefix('+') {
			self.added += 1;
			self.feed_content(content, true);
		} else if let Some(content) = line.strip_prefix(' ') {
			self.feed_content(content, false);
		}
	}

	fn feed_content(&mut self, content: &str, added: bool) {
		let after_old_header =
			std::mem::replace(&mut self.after_old_header, content.starts_with("--- "));
		if let Some(target) = content.strip_prefix("+++ ").filter(|_| after_old_header) {
			self.target = Some(git::unquote_path(target));
		} else if added
			&& content.starts_with('+')
			&& let Some(target) = &self.target
		{
			*self.target_lines.entry(target.clone()).or_default() += 1;
		}
	}

	fn flush(&mut self) {
		let target_lines = std::mem::take(&mut self.target_lines);
		let added = std::mem::take(&mut self.added);
		let file = self.file.take();
		self.in_hunk = false;
		self.after_old_header = false;
		self.target = None;
		let within_cap = (1..=MAX_ADDED_PER_FILE).contains(&added);
		if let (Some(week), Some(path), true) = (self.week, file, within_cap) {
			self.touches
				.extend(target_lines.into_iter().map(|(target, added)| Touch {
					week,
					path: path.clone(),
					added,
					patched: Some(target),
				}));
		}
	}
}

fn parse_patch_touches(
	reader: impl BufRead,
	wanted: &HashSet<String>,
) -> std::io::Result<Vec<Touch>> {
	let mut parser = PatchParser {
		wanted,
		week: None,
		file: None,
		in_hunk: false,
		after_old_header: false,
		target: None,
		added: 0,
		target_lines: BTreeMap::new(),
		touches: Vec::new(),
	};
	feed_tokens(reader, b'\n', |line| parser.feed(line)).map(|()| {
		parser.flush();
		parser.touches
	})
}

struct Numstat<'a> {
	added: Option<u64>,
	path: &'a str,
}

fn split_numstat(token: &str) -> Option<Numstat<'_>> {
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

	const WEEK_ONE_TS: i64 = 1_795_392_000;
	const WEEK_SECONDS: i64 = 7 * 24 * 3600;

	fn lines(count: usize) -> String {
		"x\n".repeat(count)
	}

	fn authors(list: &[&str]) -> Vec<String> {
		list.iter().map(|author| (*author).to_owned()).collect()
	}

	fn week(year: i16, month: i8, day: i8) -> Week {
		Date::constant(year, month, day)
	}

	fn bucket(entries: &[(&'static str, u64)]) -> BTreeMap<&'static str, u64> {
		entries.iter().copied().collect()
	}

	#[test]
	fn week_of_is_monday_utc() {
		assert_eq!(week_of(WEEK_ONE_TS), week(2026, 11, 23));
		assert_eq!(week_of(WEEK_ONE_TS + 7 * 86_400 - 60), week(2026, 11, 23));
		assert_eq!(week_of(WEEK_ONE_TS + 7 * 86_400), week(2026, 11, 30));
	}

	#[test]
	fn counts_added_lines_per_language_per_week() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		repo.commit("other@x", WEEK_ONE_TS + 1, &[("b.rs", Some(&lines(100)))]);
		repo.commit(
			"me@x",
			WEEK_ONE_TS + 2 * WEEK_SECONDS,
			&[("c.c", Some(&lines(4)))],
		);

		let (weeks, head) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(head, Some(repo.head()));
		assert_eq!(
			weeks,
			Weeks::from([
				(week(2026, 11, 23), bucket(&[("Rust", 10)])),
				(week(2026, 12, 7), bucket(&[("C", 4)])),
			])
		);
	}

	#[test]
	fn skips_binary_lockfiles_attributes_and_huge_files() {
		let repo = FixtureRepo::new();
		repo.commit(
			"me@x",
			WEEK_ONE_TS,
			&[
				(
					".gitattributes",
					Some("libft/** linguist-vendored\n*.inc linguist-language=C\n"),
				),
				("logo.png", Some("\0\0\0png")),
				("Cargo.lock", Some(&lines(50))),
				("libft/ft.c", Some(&lines(50))),
				("huge.c", Some(&lines(5001))),
				("gen.inc", Some(&lines(3))),
			],
		);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("C", 3)]))])
		);
	}

	#[test]
	fn cap_is_per_file_and_inclusive() {
		let repo = FixtureRepo::new();
		repo.commit(
			"me@x",
			WEEK_ONE_TS,
			&[
				("a.c", Some(&lines(5000))),
				("b.c", Some(&lines(5000))),
				("c.c", Some(&lines(5001))),
			],
		);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("C", 10_000)]))])
		);
	}

	const PATCH: &str = "From 0 Mon Sep 17 00:00:00 2001\nSubject: [PATCH] fix\n\n---\ndiff --git a/src/a.c b/src/a.c\nindex 1..2 100644\n--- a/src/a.c\n+++ b/src/a.c\n@@ -1,2 +1,4 @@\n int a;\n-int b;\n+int c;\n+int d;\n+int e;\ndiff --git a/lib.rs b/lib.rs\n--- a/lib.rs\n+++ b/lib.rs\t2026-01-01\n@@ -1 +1,2 @@\n+fn a() {}\n+fn b() {}\n fn c() {}\n";

	fn patch_adding(target: &str, count: usize) -> String {
		format!(
			"--- a/{target}\n+++ b/{target}\n@@ -0,0 +1 @@\n{}",
			"+x\n".repeat(count)
		)
	}

	#[test]
	fn patch_lines_count_for_the_patched_language() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("patches/fix.patch", Some(PATCH))]);
		repo.commit("other@x", WEEK_ONE_TS + 1, &[("other.patch", Some(PATCH))]);
		let grown = PATCH.replace("+int e;\n", "+int e;\n+int f;\n");
		repo.commit(
			"me@x",
			WEEK_ONE_TS + WEEK_SECONDS,
			&[
				("patches/fix.patch", Some(&grown)),
				("UP.PATCH", Some(&patch_adding("x.py", 2))),
			],
		);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([
				(week(2026, 11, 23), bucket(&[("C", 3), ("Rust", 2)])),
				(week(2026, 11, 30), bucket(&[("C", 1), ("Python", 2)])),
			])
		);
	}

	#[test]
	fn vendored_deleted_and_huge_patches_count_nothing() {
		let repo = FixtureRepo::new();
		repo.commit(
			"me@x",
			WEEK_ONE_TS,
			&[
				(".gitattributes", Some("vendor/** linguist-vendored\n")),
				("vendor/x.patch", Some(PATCH)),
				("huge.patch", Some(&patch_adding("a.c", 5000))),
				("gone.patch", Some(&patch_adding("/dev/null", 3))),
			],
		);
		repo.commit("me@x", WEEK_ONE_TS + WEEK_SECONDS, &[("gone.patch", None)]);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert!(weeks.is_empty(), "{weeks:?}");
	}

	#[test]
	fn quoted_and_spaced_patch_names_follow_their_attributes() {
		let repo = FixtureRepo::new();
		repo.commit(
			"me@x",
			WEEK_ONE_TS,
			&[
				(
					".gitattributes",
					Some("vendored*.patch linguist-vendored\n"),
				),
				("vendored a.patch", Some(PATCH)),
				("vendored-\u{e9}t\u{e9}.patch", Some(PATCH)),
				("my fix.patch", Some(&patch_adding("x.py", 2))),
			],
		);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Python", 2)]))])
		);
	}

	#[test]
	fn gitattributes_override_still_wins() {
		let repo = FixtureRepo::new();
		repo.commit(
			"me@x",
			WEEK_ONE_TS,
			&[
				(
					".gitattributes",
					Some("src/** linguist-language=Boo\n*.cfgx linguist-language=json\n*.txtx linguist-language=Text\n"),
				),
				("src/yasl_do", Some(&lines(4))),
				("src/yasl_hw.c", Some(&lines(1))),
				("a.cfgx", Some(&lines(2))),
				("a.txtx", Some(&lines(5))),
			],
		);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Boo", 5), ("JSON", 2)]))])
		);
	}

	#[test]
	fn rename_counts_zero() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		repo.mv("a.rs", "dir/b.rs", WEEK_ONE_TS + WEEK_SECONDS);

		let (weeks, _) = weekly_added(
			repo.path(),
			None,
			&authors(&["committer@example.com"]),
			&shipped_detector(),
		)
		.unwrap();

		assert!(weeks.is_empty());
	}

	#[test]
	fn rename_with_edit_counts_added_lines_under_new_path() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		repo.commit(
			"me@x",
			WEEK_ONE_TS + WEEK_SECONDS,
			&[("a.rs", None), ("b.c", Some(&format!("{}y\n", lines(10))))],
		);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([
				(week(2026, 11, 23), bucket(&[("Rust", 10)])),
				(week(2026, 11, 30), bucket(&[("C", 1)])),
			])
		);
	}

	#[test]
	fn since_reads_only_new_commits() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		let first = repo.head();
		repo.commit("me@x", WEEK_ONE_TS + 1, &[("b.rs", Some(&lines(7)))]);
		let bare = repo.bare_clone();

		let (weeks, head) = weekly_added(
			bare.path(),
			Some(&first),
			&authors(&["me@x"]),
			&shipped_detector(),
		)
		.unwrap();

		assert_eq!(head, Some(repo.head()));
		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Rust", 7)]))])
		);
	}

	#[test]
	fn since_head_gives_nothing() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);

		let (weeks, head) = weekly_added(
			repo.path(),
			Some(&repo.head()),
			&authors(&["me@x"]),
			&shipped_detector(),
		)
		.unwrap();

		assert!(weeks.is_empty());
		assert_eq!(head, Some(repo.head()));
	}

	#[test]
	fn empty_repo_returns_none_head() {
		let repo = FixtureRepo::new();
		let bare = repo.bare_clone();
		for path in [repo.path(), bare.path()] {
			let (weeks, head) =
				weekly_added(path, None, &authors(&["me@x"]), &shipped_detector()).unwrap();
			assert!(weeks.is_empty());
			assert_eq!(head, None);
		}
	}

	#[test]
	fn author_email_match_ignores_case() {
		let repo = FixtureRepo::new();
		repo.commit("Me@X.com", WEEK_ONE_TS, &[("a.rs", Some(&lines(2)))]);

		let (weeks, _) = weekly_added(
			repo.path(),
			None,
			&authors(&["mE@x.COM"]),
			&shipped_detector(),
		)
		.unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Rust", 2)]))])
		);
	}

	#[test]
	fn same_week_commits_accumulate() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(2)))]);
		repo.commit("me@x", WEEK_ONE_TS + 60, &[("b.rs", Some(&lines(3)))]);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Rust", 5)]))])
		);
	}

	#[test]
	fn commit_without_file_changes_is_skipped() {
		let repo = FixtureRepo::new();
		repo.empty_commit("me@x", WEEK_ONE_TS);
		repo.commit("me@x", WEEK_ONE_TS + 1, &[("a.rs", Some(&lines(2)))]);
		repo.empty_commit("me@x", WEEK_ONE_TS + 2);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["me@x"]), &shipped_detector()).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Rust", 2)]))])
		);
	}

	#[test]
	fn fails_outside_a_repository() {
		let dir = tempfile::TempDir::new().unwrap();
		assert!(weekly_added(dir.path(), None, &[], &shipped_detector()).is_err());
	}

	#[test]
	fn unknown_since_is_an_error() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(2)))]);
		let message = weekly_added(repo.path(), Some("deadbeef"), &[], &shipped_detector())
			.unwrap_err()
			.to_string();
		assert!(message.starts_with("git log in "), "{message}");
	}

	#[test]
	fn truncated_output_yields_no_touches() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		assert!(
			parse_touches(&b"sha\0me@x"[..], &wanted)
				.unwrap()
				.is_empty()
		);
		assert!(
			parse_touches(&b"sha\0me@x\x001\0\n"[..], &wanted)
				.unwrap()
				.is_empty()
		);
	}

	#[test]
	fn invalid_timestamp_skips_the_commit() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		let output = b"sha\0me@x\0soon\0\n1\t0\ta.rs\0";
		assert!(parse_touches(&output[..], &wanted).unwrap().is_empty());
	}

	struct BrokenPipe;

	impl std::io::Read for BrokenPipe {
		fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
			Err(std::io::Error::other("broken pipe"))
		}
	}

	#[test]
	fn patch_lines_without_commit_or_target_count_nothing() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		let orphan = b"+++ a.patch\n@@ -0,0 +1 @@\n++++ b/a.c\n++x\n";
		let headerless = b"\0me@x\n+++ a.patch\n@@ -0,0 +1 @@\n++++ b/a.c\n++x\n";
		let targetless = b"\0me@x\x00100\n+++ a.patch\n@@ -0,0 +1 @@\n++x\n";
		for output in [&orphan[..], &headerless[..], &targetless[..]] {
			assert!(parse_patch_touches(output, &wanted).unwrap().is_empty());
		}
		let counted =
			b"\0me@x\x00100\r\n+++ a.patch\n@@ -0,0 +1 @@\n+--- a/a.c\n++++ b/a.c\r\n++x\r\n\\ No newline\n";
		let touches = parse_patch_touches(&counted[..], &wanted).unwrap();
		assert_eq!(touches.len(), 1);
		assert_eq!(touches[0].patched.as_deref(), Some("b/a.c"));
	}

	#[test]
	fn inner_added_line_starting_with_plus_plus_keeps_its_target() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		let output = b"\0me@x\x00100\n+++ a.patch\n@@ -0,0 +1 @@\n+--- a/a.c\n++++ b/a.c\n++++ x\n+ y\n++z\n";
		let touches = parse_patch_touches(&output[..], &wanted).unwrap();
		assert_eq!(touches.len(), 1);
		assert_eq!(touches[0].patched.as_deref(), Some("b/a.c"));
		assert_eq!(touches[0].added, 2);
	}

	#[test]
	fn patch_read_error_is_an_error_not_a_partial_count() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		let valid = &b"\0me@x\x00100\n+++ a.patch\n@@ -0,0 +1 @@\n++++ b/a.c\n"[..];
		let reader = BufReader::new(std::io::Read::chain(valid, BrokenPipe));

		let error = parse_patch_touches(reader, &wanted).unwrap_err();

		assert_eq!(error.to_string(), "reading git log output: broken pipe");
	}

	#[test]
	fn read_error_mid_stream_is_an_error_not_a_partial_count() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		let valid = &b"sha\0me@x\x00100\0\n1\t0\ta.rs\0"[..];
		let reader = BufReader::new(std::io::Read::chain(valid, BrokenPipe));

		let error = parse_touches(reader, &wanted).unwrap_err();

		assert_eq!(error.to_string(), "reading git log output: broken pipe");
	}
}
