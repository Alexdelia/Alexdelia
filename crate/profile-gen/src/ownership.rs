use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::error::Result;
use crate::git::{self, output, run};
use crate::history::{MAX_ADDED_PER_FILE, is_patch, split_numstat};

pub(crate) const BLAME_OPTIONS: [&str; 3] = ["-w", "-M", "-C"];
const IGNORE_REVS_FILE: &str = ".git-blame-ignore-revs";
const SHA_HEX_LENGTH: usize = 40;
const PATCH_TARGET_PREFIX: &str = "b/";
const DELETED_TARGET: &str = "/dev/null";
const COMMIT_EMAILS_FORMAT: &str = "--format=%H%n%ae%n%(trailers:key=Co-authored-by,valueonly)";

#[derive(Debug, Default)]
pub struct Ownership {
	reachable: HashSet<String>,
	mine: HashSet<String>,
	capped: HashMap<String, HashSet<String>>,
}

impl Ownership {
	pub(crate) fn is_mine(&self, sha: &str) -> bool {
		self.mine.contains(sha)
	}

	fn counts(&self, sha: &str, filename: &str) -> bool {
		self.is_mine(sha)
			&& !self
				.capped
				.get(sha)
				.is_some_and(|paths| paths.contains(filename))
	}
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Owned {
	pub plain: u64,
	pub patched: BTreeMap<String, u64>,
}

pub fn ownership(repo: &Path, authors: &[String]) -> Result<Ownership> {
	if git::head_sha(repo)?.is_none() {
		return Ok(Ownership::default());
	}
	let wanted: HashSet<String> = authors.iter().map(|author| author.to_lowercase()).collect();
	run(repo, &["log", "-z", COMMIT_EMAILS_FORMAT, "HEAD"])
		.map(|log| split_commits(&String::from_utf8_lossy(&log), &wanted))
		.and_then(|(reachable, mine)| {
			run(
				repo,
				&["log", "--numstat", "-M", "-z", "--format=%H", "HEAD"],
			)
			.map(|log| Ownership {
				capped: capped_pairs(&String::from_utf8_lossy(&log), &mine),
				reachable,
				mine,
			})
		})
}

fn split_commits(log: &str, wanted: &HashSet<String>) -> (HashSet<String>, HashSet<String>) {
	let mut reachable = HashSet::new();
	let mut mine = HashSet::new();
	for record in log.split('\0') {
		let mut lines = record.lines();
		if let (Some(sha), Some(author)) = (lines.next(), lines.next()) {
			let mut emails = std::iter::once(author).chain(lines.filter_map(trailer_email));
			if emails.any(|email| wanted.contains(&email.to_lowercase())) {
				mine.insert(sha.to_owned());
			}
			reachable.insert(sha.to_owned());
		}
	}
	(reachable, mine)
}

fn trailer_email(value: &str) -> Option<&str> {
	value
		.rsplit_once('<')
		.and_then(|(_, rest)| rest.split_once('>'))
		.map(|(email, _)| email)
}

enum NumstatToken {
	Entry,
	RenamedFrom(Option<u64>),
	RenamedTo(Option<u64>),
}

fn capped_pairs(log: &str, mine: &HashSet<String>) -> HashMap<String, HashSet<String>> {
	let mut capped: HashMap<String, HashSet<String>> = HashMap::new();
	let mut record = |sha: &str, path: &str, added: Option<u64>| {
		if added.is_some_and(|added| added > MAX_ADDED_PER_FILE) && mine.contains(sha) {
			capped
				.entry(sha.to_owned())
				.or_default()
				.insert(path.to_owned());
		}
	};
	let mut sha = "";
	let mut expect = NumstatToken::Entry;
	for token in log.split('\0') {
		expect = match expect {
			NumstatToken::RenamedFrom(added) => NumstatToken::RenamedTo(added),
			NumstatToken::RenamedTo(added) => {
				record(sha, token, added);
				NumstatToken::Entry
			}
			NumstatToken::Entry => match split_numstat(token) {
				None => {
					sha = token;
					NumstatToken::Entry
				}
				Some(entry) if entry.path.is_empty() => NumstatToken::RenamedFrom(entry.added),
				Some(entry) => {
					record(sha, entry.path, entry.added);
					NumstatToken::Entry
				}
			},
		};
	}
	capped
}

pub fn owned_lines(repo: &Path, commit: &str, path: &str, ownership: &Ownership) -> Result<Owned> {
	ignored_revs(repo, commit, ownership)
		.and_then(|ignored| owned_lines_ignoring(repo, commit, path, ownership, &ignored))
}

pub(crate) fn owned_lines_ignoring(
	repo: &Path,
	commit: &str,
	path: &str,
	ownership: &Ownership,
	ignored: &[String],
) -> Result<Owned> {
	let mut args = vec!["blame", "--line-porcelain"];
	args.extend(BLAME_OPTIONS);
	for rev in ignored {
		args.extend(["--ignore-rev", rev]);
	}
	args.extend([commit, "--", path]);
	run(repo, &args).map(|porcelain| tally(&porcelain, is_patch(path), ownership))
}

pub(crate) fn ignored_revs(
	repo: &Path,
	commit: &str,
	ownership: &Ownership,
) -> Result<Vec<String>> {
	let blob = format!("{commit}:{IGNORE_REVS_FILE}");
	let args = ["rev-parse", "--verify", "--quiet", &blob];
	output(repo, &args).and_then(|output| match output.status.code() {
		Some(0) => run(repo, &["cat-file", "blob", &blob])
			.map(|listed| reachable_revs(&String::from_utf8_lossy(&listed), ownership)),
		Some(1) => Ok(Vec::new()),
		_ => Err(git::failure(repo, &args, &output.stderr)),
	})
}

fn reachable_revs(listed: &str, ownership: &Ownership) -> Vec<String> {
	listed
		.lines()
		.map(str::trim)
		.filter(|line| {
			line.len() == SHA_HEX_LENGTH && line.bytes().all(|byte| byte.is_ascii_hexdigit())
		})
		.map(str::to_ascii_lowercase)
		.filter(|rev| ownership.reachable.contains(rev))
		.collect()
}

fn tally(porcelain: &[u8], patch: bool, ownership: &Ownership) -> Owned {
	let mut owned = Owned::default();
	let mut targets = PatchTargets::default();
	for_each_blamed(porcelain, |sha, filename, content| {
		let counted = ownership.counts(sha, filename);
		if !patch {
			owned.plain += u64::from(counted);
		} else if let Some(target) = targets.added_line_target(&String::from_utf8_lossy(content)) {
			*owned.patched.entry(target.to_owned()).or_default() += u64::from(counted);
		}
	});
	owned.patched.retain(|_, lines| *lines > 0);
	owned
}

fn for_each_blamed(porcelain: &[u8], mut visit: impl FnMut(&str, &str, &[u8])) {
	let mut sha = String::new();
	let mut filename = String::new();
	let mut at_header = true;
	for line in porcelain.split(|&byte| byte == b'\n') {
		if let Some(content) = line.strip_prefix(b"\t") {
			visit(&sha, &filename, content);
			at_header = true;
		} else if at_header {
			let header_sha = line.split(|&byte| byte == b' ').next().unwrap_or_default();
			sha = String::from_utf8_lossy(header_sha).into_owned();
			at_header = false;
		} else if let Some(name) = line.strip_prefix(b"filename ") {
			filename = git::unquote_path(&String::from_utf8_lossy(name));
		}
	}
}

#[derive(Default)]
struct PatchTargets {
	after_old_header: bool,
	target: Option<String>,
}

impl PatchTargets {
	fn added_line_target(&mut self, content: &str) -> Option<&str> {
		let content = content.trim_end_matches('\r');
		let after_old_header =
			std::mem::replace(&mut self.after_old_header, content.starts_with("--- "));
		if let Some(printed) = content.strip_prefix("+++ ").filter(|_| after_old_header) {
			self.target = patch_target(printed);
			None
		} else if content.starts_with('+') {
			self.target.as_deref()
		} else {
			self.target = self.target.take().filter(|_| continues_hunk(content));
			None
		}
	}
}

fn continues_hunk(content: &str) -> bool {
	content.is_empty() || content.starts_with([' ', '-', '\\', '@'])
}

fn patch_target(printed: &str) -> Option<String> {
	let path = git::unquote_path(printed);
	(path != DELETED_TARGET).then(|| {
		path.strip_prefix(PATCH_TARGET_PREFIX)
			.unwrap_or(&path)
			.to_owned()
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fixture::FixtureRepo;

	const ME: &str = "me@x";
	const FRIEND: &str = "friend@x";

	fn mine() -> Vec<String> {
		vec![ME.to_owned()]
	}

	fn plain(repo: &Path, commit: &str, path: &str) -> u64 {
		let ownership = ownership(repo, &mine()).unwrap();
		let owned = owned_lines(repo, commit, path, &ownership).unwrap();
		assert!(owned.patched.is_empty(), "{owned:?}");
		owned.plain
	}

	fn numbered(prefix: &str, range: std::ops::Range<usize>) -> String {
		range
			.map(|index| {
				format!("{prefix}int value_{index}_alpha = compute_value({index}, 2, 3);\n")
			})
			.collect::<Vec<_>>()
			.concat()
	}

	#[test]
	fn own_commit_lines_count() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("a.c", Some(&numbered("", 0..3)))]);

		assert_eq!(plain(repo.path(), &repo.head(), "a.c"), 3);
	}

	#[test]
	fn other_author_lines_do_not() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("a.c", Some(&numbered("", 0..3)))]);
		repo.commit(
			FRIEND,
			2,
			&[
				("a.c", Some(&numbered("", 0..5))),
				("b.c", Some(&numbered("other_", 0..4))),
			],
		);
		let bare = repo.bare_clone();

		assert_eq!(plain(bare.path(), &repo.head(), "a.c"), 3);
		assert_eq!(plain(bare.path(), &repo.head(), "b.c"), 0);
	}

	#[test]
	fn author_match_ignores_case() {
		let repo = FixtureRepo::new();
		repo.commit("Me@X", 1, &[("a.c", Some(&numbered("", 0..2)))]);
		let ownership = ownership(repo.path(), &["mE@x".to_owned()]).unwrap();

		let owned = owned_lines(repo.path(), &repo.head(), "a.c", &ownership).unwrap();

		assert_eq!(owned.plain, 2);
	}

	#[test]
	fn co_authored_commit_counts() {
		let repo = FixtureRepo::new();
		repo.commit_with_message(
			FRIEND,
			1,
			"pair\n\nCo-authored-by: Someone <someone@x>\nCo-authored-by: Me <ME@X>\n",
			&[("a.c", Some(&numbered("", 0..4)))],
		);
		repo.commit_with_message(
			FRIEND,
			2,
			"alone\n\nCo-authored-by: me@x\n",
			&[("b.c", Some(&numbered("", 0..2)))],
		);

		assert_eq!(plain(repo.path(), &repo.head(), "a.c"), 4);
		assert_eq!(plain(repo.path(), &repo.head(), "b.c"), 0);
	}

	#[test]
	fn merge_resolution_lines_count() {
		let repo = FixtureRepo::new();
		repo.commit(FRIEND, 1, &[("a.c", Some(&numbered("", 0..2)))]);
		repo.branch("side");
		repo.commit(FRIEND, 2, &[("b.c", Some(&numbered("", 0..2)))]);
		repo.switch("main");
		repo.commit(FRIEND, 3, &[("c.c", Some(&numbered("", 0..2)))]);
		repo.merge(ME, 4, "side", &[("a.c", Some(&numbered("", 0..5)))]);

		assert_eq!(plain(repo.path(), &repo.head(), "a.c"), 3);
	}

	#[test]
	fn teammate_reindent_keeps_my_lines() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("a.c", Some(&numbered("", 0..5)))]);
		repo.commit(FRIEND, 2, &[("a.c", Some(&numbered("\t\t", 0..5)))]);

		assert_eq!(plain(repo.path(), &repo.head(), "a.c"), 5);
	}

	#[test]
	fn teammate_moving_my_lines_to_new_file_keeps_them() {
		let repo = FixtureRepo::new();
		let kept = numbered("", 0..2);
		let moved = numbered("", 2..12);
		repo.commit(ME, 1, &[("a.c", Some(&format!("{kept}{moved}")))]);
		repo.commit(FRIEND, 2, &[("a.c", Some(&kept)), ("b.c", Some(&moved))]);

		assert_eq!(plain(repo.path(), &repo.head(), "b.c"), 10);
		assert_eq!(plain(repo.path(), &repo.head(), "a.c"), 2);
	}

	#[test]
	fn renamed_file_keeps_my_lines() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("a.c", Some(&numbered("", 0..6)))]);
		repo.mv("a.c", "dir/b.c", 2);

		assert_eq!(plain(repo.path(), &repo.head(), "dir/b.c"), 6);
	}

	#[test]
	fn ignore_revs_file_skips_formatter_commit() {
		let repo = FixtureRepo::new();
		let wrapped = (0..5)
			.map(|index| {
				format!("call_function_number_{index}(first_argument, second_argument);\n")
			})
			.collect::<Vec<_>>()
			.concat();
		repo.commit(ME, 1, &[("a.c", Some(&wrapped))]);
		repo.commit(
			FRIEND,
			2,
			&[
				("a.c", Some(&wrapped.replace(", ", ",\t/**/"))),
				("b.c", Some("int b;\n")),
			],
		);
		let formatter = repo.head();
		let ignore_list = format!(
			"# formatter\n{formatter}\n{}\nnot-a-sha\n{}\n",
			"0123456789abcdef0123456789abcdef01234567",
			&formatter[..12]
		);
		repo.commit(FRIEND, 3, &[(".git-blame-ignore-revs", Some(&ignore_list))]);
		let bare = repo.bare_clone();

		assert_eq!(plain(bare.path(), &formatter, "a.c"), 0);
		assert_eq!(plain(bare.path(), &repo.head(), "a.c"), 5);
	}

	#[test]
	fn capped_commit_lines_do_not_count() {
		let repo = FixtureRepo::new();
		repo.commit(FRIEND, 1, &[("seed.c", Some("int seed;\n"))]);
		repo.commit(
			ME,
			2,
			&[
				("big.c", Some(&numbered("", 0..5001))),
				("edge.c", Some(&numbered("", 0..5000))),
				("logo.png", Some("\0\0\0png")),
			],
		);
		repo.commit(
			ME,
			3,
			&[(
				"big.c",
				Some(&format!(
					"{}{}",
					numbered("", 0..5001),
					numbered("tail_", 0..2)
				)),
			)],
		);

		assert_eq!(plain(repo.path(), &repo.head(), "big.c"), 2);
		assert_eq!(plain(repo.path(), &repo.head(), "edge.c"), 5000);
	}

	const FIX_PATCH: &str = "From 0 Mon Sep 17 00:00:00 2001\nSubject: [PATCH] fix\n\n+not under a target\n---\ndiff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1,3 @@\n fn a() {}\n-fn old() {}\n+fn b() {}\n+fn c() {}\n+++ looks like a header\ndiff --git a/b.py b/b.py\n--- a/b.py\n+++ \"b/b.py\"\t2026-01-01\n@@ -0,0 +1 @@\n+x = 1\n";

	#[test]
	fn patch_lines_split_by_target() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("fix.PATCH", Some(FIX_PATCH))]);
		repo.commit(
			FRIEND,
			2,
			&[("fix.PATCH", Some(&format!("{FIX_PATCH}+y = 2\n")))],
		);
		let ownership = ownership(repo.path(), &mine()).unwrap();

		let owned = owned_lines(repo.path(), &repo.head(), "fix.PATCH", &ownership).unwrap();

		assert_eq!(
			owned,
			Owned {
				plain: 0,
				patched: BTreeMap::from([("a.rs".to_owned(), 3), ("b.py".to_owned(), 1)]),
			}
		);
	}

	fn my_patch_lines(patch: &str) -> BTreeMap<String, u64> {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("series.patch", Some(patch))]);
		let ownership = ownership(repo.path(), &mine()).unwrap();
		let owned = owned_lines(repo.path(), &repo.head(), "series.patch", &ownership).unwrap();
		assert_eq!(owned.plain, 0);
		owned.patched
	}

	const SERIES_PATCH: &str = "From 1 Mon Sep 17 00:00:00 2001\nSubject: [PATCH 1/2] one\n\n---\ndiff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,2 @@\n+fn a() {}\n@@ -9,0 +10 @@\n+fn z() {}\ndiff --git a/img.png b/img.png\n+stray after a diff header\n-- \n2.40.0\n\nFrom 2 Mon Sep 17 00:00:00 2001\nSubject: [PATCH 2/2] two\n\n+body line looking added\n---\ndiff --git a/b.py b/b.py\n--- a/b.py\n+++ b/b.py\n@@ -0,0 +1 @@\n+x = 1\n";

	#[test]
	fn patch_target_resets_at_next_diff_header() {
		assert_eq!(
			my_patch_lines(SERIES_PATCH),
			BTreeMap::from([("a.rs".to_owned(), 2), ("b.py".to_owned(), 1)])
		);
	}

	const DELETED_AND_TIMESTAMPED_PATCH: &str = "--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-fn gone() {}\n+stray under a deleted file\n--- a/c.rs\t2026-01-01 10:00:00\n+++ b/c.rs\t2026-01-01 10:00:00\n@@ -0,0 +1 @@\n+fn c() {}\n";

	#[test]
	fn deleted_target_never_counts_and_timestamp_is_dropped() {
		assert_eq!(
			my_patch_lines(DELETED_AND_TIMESTAMPED_PATCH),
			BTreeMap::from([("c.rs".to_owned(), 1)])
		);
	}

	#[test]
	fn capped_path_renamed_later_stays_capped() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("big.c", Some(&numbered("", 0..5001)))]);
		repo.commit(
			FRIEND,
			2,
			&[("big.c", None), ("huge.c", Some(&numbered("", 0..5001)))],
		);

		assert_eq!(plain(repo.path(), &repo.head(), "huge.c"), 0);
	}

	#[test]
	fn empty_repo_owns_nothing() {
		let repo = FixtureRepo::new();
		let bare = repo.bare_clone();

		let ownership = ownership(bare.path(), &mine()).unwrap();

		assert!(ownership.reachable.is_empty());
		assert!(ownership.mine.is_empty());
		assert!(ownership.capped.is_empty());
		assert!(owned_lines(bare.path(), "HEAD", "a.c", &ownership).is_err());
	}

	#[test]
	fn missing_path_is_an_error() {
		let repo = FixtureRepo::new();
		repo.commit(ME, 1, &[("a.c", Some("int a;\n"))]);
		let ownership = ownership(repo.path(), &mine()).unwrap();

		let message = owned_lines(repo.path(), &repo.head(), "gone.c", &ownership)
			.unwrap_err()
			.to_string();

		assert!(message.starts_with("git blame in "), "{message}");
	}

	#[test]
	fn outside_a_repository_is_an_error() {
		let dir = tempfile::TempDir::new().unwrap();
		assert!(ownership(dir.path(), &mine()).is_err());
		assert!(owned_lines(dir.path(), "HEAD", "a.c", &Ownership::default()).is_err());
	}
}
