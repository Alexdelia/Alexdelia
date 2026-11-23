use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Output, Stdio};

use jiff::civil::Date;
use jiff::tz::Offset;
use jiff::{Span, Timestamp};

use crate::attributes::{Attribute, attributes};
use crate::error::Result;
use crate::git;
use crate::language::{Language, language_for_path};

pub type Week = Date;
pub type Weeks = BTreeMap<Week, BTreeMap<&'static str, u64>>;

const MAX_ADDED_PER_FILE: u64 = 5000;

#[derive(Debug)]
struct Touch {
	week: Week,
	path: String,
	added: u64,
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
) -> Result<(Weeks, Option<String>)> {
	let Some(head) = git::head_sha(repo)? else {
		return Ok((Weeks::new(), None));
	};
	let wanted: HashSet<String> = authors.iter().map(|author| author.to_lowercase()).collect();
	read_touches(repo, since, &wanted)
		.and_then(|touches| {
			let paths: Vec<String> = touches
				.iter()
				.map(|touch| touch.path.clone())
				.collect::<HashSet<_>>()
				.into_iter()
				.collect();
			attributes(repo, &paths).map(|attribute_map| tally(&touches, &attribute_map))
		})
		.map(|weeks| (weeks, Some(head)))
}

fn tally(touches: &[Touch], attribute_map: &std::collections::HashMap<String, Attribute>) -> Weeks {
	let mut weeks = Weeks::new();
	for touch in touches {
		if let Some(language) = resolve_language(&touch.path, attribute_map[&touch.path]) {
			*weeks
				.entry(touch.week)
				.or_default()
				.entry(language.name)
				.or_default() += touch.added;
		}
	}
	weeks
}

fn resolve_language(path: &str, attribute: Attribute) -> Option<Language> {
	match attribute {
		Attribute::Skip => None,
		Attribute::Language(language) => Some(language),
		Attribute::Default => language_for_path(path),
	}
}

fn read_touches(repo: &Path, since: Option<&str>, wanted: &HashSet<String>) -> Result<Vec<Touch>> {
	let range = since.map_or_else(|| "HEAD".to_owned(), |sha| format!("{sha}..HEAD"));
	let args = [
		"log",
		"--no-merges",
		"-M",
		"--numstat",
		"-z",
		"--format=%H%x00%ae%x00%at",
		&range,
	];
	git::command(repo)
		.args(args)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.and_then(|child| collect(child, wanted))
		.map_err(Into::into)
		.and_then(|(touches, output)| {
			if output.status.success() {
				Ok(touches)
			} else {
				Err(git::failure(repo, &args, &output.stderr))
			}
		})
}

fn collect(mut child: Child, wanted: &HashSet<String>) -> std::io::Result<(Vec<Touch>, Output)> {
	let touches = child
		.stdout
		.take()
		.map(|stdout| parse_touches(BufReader::new(stdout), wanted))
		.transpose()
		.map(Option::unwrap_or_default);
	let exit = child.wait_with_output();
	touches.and_then(|touches| exit.map(|output| (touches, output)))
}

fn read_token(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
	let mut bytes = Vec::new();
	reader.read_until(0, &mut bytes).map(|read| {
		(read > 0).then(|| {
			if bytes.last() == Some(&0) {
				bytes.pop();
			}
			String::from_utf8_lossy(&bytes).into_owned()
		})
	})
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
				week: token.parse().ok().map(week_of).filter(|_| counted),
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
			});
		}
	}
}

fn parse_touches(
	mut reader: impl BufRead,
	wanted: &HashSet<String>,
) -> std::io::Result<Vec<Touch>> {
	let mut parser = Parser {
		wanted,
		phase: Phase::Sha,
		touches: Vec::new(),
	};
	let mut failure = None;
	std::iter::from_fn(|| match read_token(&mut reader) {
		Ok(token) => token,
		Err(error) => {
			failure = Some(std::io::Error::new(
				error.kind(),
				format!("reading git log output: {error}"),
			));
			None
		}
	})
	.for_each(|token| parser.feed(&token));
	failure.map_or(Ok(parser.touches), Err)
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
	use crate::fixture::FixtureRepo;

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

		let (weeks, head) = weekly_added(repo.path(), None, &authors(&["me@x"])).unwrap();

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

		let (weeks, _) = weekly_added(repo.path(), None, &authors(&["me@x"])).unwrap();

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

		let (weeks, _) = weekly_added(repo.path(), None, &authors(&["me@x"])).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("C", 10_000)]))])
		);
	}

	#[test]
	fn rename_counts_zero() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		repo.mv("a.rs", "dir/b.rs", WEEK_ONE_TS + WEEK_SECONDS);

		let (weeks, _) =
			weekly_added(repo.path(), None, &authors(&["committer@example.com"])).unwrap();

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

		let (weeks, _) = weekly_added(repo.path(), None, &authors(&["me@x"])).unwrap();

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

		let (weeks, head) = weekly_added(bare.path(), Some(&first), &authors(&["me@x"])).unwrap();

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

		let (weeks, head) =
			weekly_added(repo.path(), Some(&repo.head()), &authors(&["me@x"])).unwrap();

		assert!(weeks.is_empty());
		assert_eq!(head, Some(repo.head()));
	}

	#[test]
	fn empty_repo_returns_none_head() {
		let repo = FixtureRepo::new();
		let bare = repo.bare_clone();
		for path in [repo.path(), bare.path()] {
			let (weeks, head) = weekly_added(path, None, &authors(&["me@x"])).unwrap();
			assert!(weeks.is_empty());
			assert_eq!(head, None);
		}
	}

	#[test]
	fn author_email_match_ignores_case() {
		let repo = FixtureRepo::new();
		repo.commit("Me@X.com", WEEK_ONE_TS, &[("a.rs", Some(&lines(2)))]);

		let (weeks, _) = weekly_added(repo.path(), None, &authors(&["mE@x.COM"])).unwrap();

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

		let (weeks, _) = weekly_added(repo.path(), None, &authors(&["me@x"])).unwrap();

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

		let (weeks, _) = weekly_added(repo.path(), None, &authors(&["me@x"])).unwrap();

		assert_eq!(
			weeks,
			Weeks::from([(week(2026, 11, 23), bucket(&[("Rust", 2)]))])
		);
	}

	#[test]
	fn fails_outside_a_repository() {
		let dir = tempfile::TempDir::new().unwrap();
		assert!(weekly_added(dir.path(), None, &[]).is_err());
	}

	#[test]
	fn unknown_since_is_an_error() {
		let repo = FixtureRepo::new();
		repo.commit("me@x", WEEK_ONE_TS, &[("a.rs", Some(&lines(2)))]);
		let message = weekly_added(repo.path(), Some("deadbeef"), &[])
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
	fn read_error_mid_stream_is_an_error_not_a_partial_count() {
		let wanted = HashSet::from(["me@x".to_owned()]);
		let valid = &b"sha\0me@x\x00100\0\n1\t0\ta.rs\0"[..];
		let reader = BufReader::new(std::io::Read::chain(valid, BrokenPipe));

		let error = parse_touches(reader, &wanted).unwrap_err();

		assert_eq!(error.to_string(), "reading git log output: broken pipe");
	}
}
