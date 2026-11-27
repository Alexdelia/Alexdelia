use std::path::Path;

use crate::config::{Config, LanguageLists};
use crate::git;
use crate::language::Detector;
use tempfile::TempDir;

pub(crate) fn shipped_lists() -> LanguageLists {
	Config::parse(include_str!("../config.toml"))
		.unwrap()
		.languages
}

pub(crate) fn shipped_detector() -> Detector {
	Detector::new(&shipped_lists()).unwrap()
}

pub(crate) struct FixtureRepo {
	dir: TempDir,
}

impl FixtureRepo {
	pub(crate) fn new() -> Self {
		let dir = TempDir::new().unwrap();
		let repo = Self { dir };
		repo.git(&["init", "-q", "-b", "main"], "committer@example.com", 0);
		repo
	}

	pub(crate) fn path(&self) -> &Path {
		self.dir.path()
	}

	pub(crate) fn commit(
		&self,
		author_email: &str,
		unix_ts: i64,
		changes: &[(&str, Option<&str>)],
	) {
		self.commit_with_message(author_email, unix_ts, "fixture", changes);
	}

	pub(crate) fn commit_with_message(
		&self,
		author_email: &str,
		unix_ts: i64,
		message: &str,
		changes: &[(&str, Option<&str>)],
	) {
		self.write(changes);
		self.git(&["add", "-A"], author_email, unix_ts);
		self.git(&["commit", "-q", "-m", message], author_email, unix_ts);
	}

	pub(crate) fn branch(&self, name: &str) {
		self.git(&["switch", "-q", "-c", name], "committer@example.com", 0);
	}

	pub(crate) fn switch(&self, name: &str) {
		self.git(&["switch", "-q", name], "committer@example.com", 0);
	}

	pub(crate) fn fast_forward(&self, branch: &str) {
		self.git(
			&["merge", "-q", "--ff-only", branch],
			"committer@example.com",
			0,
		);
	}

	pub(crate) fn merge(
		&self,
		author_email: &str,
		unix_ts: i64,
		branch: &str,
		changes: &[(&str, Option<&str>)],
	) {
		self.git(
			&["merge", "-q", "--no-ff", "--no-commit", branch],
			author_email,
			unix_ts,
		);
		self.commit_with_message(author_email, unix_ts, "fixture merge", changes);
	}

	fn write(&self, changes: &[(&str, Option<&str>)]) {
		for (path, content) in changes {
			let full = self.path().join(path);
			match content {
				Some(content) => {
					std::fs::create_dir_all(full.parent().unwrap()).unwrap();
					std::fs::write(full, content).unwrap();
				}
				None => std::fs::remove_file(full).unwrap(),
			}
		}
	}

	pub(crate) fn empty_commit(&self, author_email: &str, unix_ts: i64) {
		self.git(
			&["commit", "-q", "--allow-empty", "-m", "fixture empty"],
			author_email,
			unix_ts,
		);
	}

	pub(crate) fn amend(&self, author_email: &str, unix_ts: i64, changes: &[(&str, &str)]) {
		for (path, content) in changes {
			std::fs::write(self.path().join(path), content).unwrap();
		}
		self.git(&["add", "-A"], author_email, unix_ts);
		self.git(
			&["commit", "-q", "--amend", "--no-edit"],
			author_email,
			unix_ts,
		);
	}

	pub(crate) fn mv(&self, from: &str, to: &str, unix_ts: i64) {
		let to_parent = self.path().join(to);
		std::fs::create_dir_all(to_parent.parent().unwrap()).unwrap();
		self.git(&["mv", from, to], "committer@example.com", unix_ts);
		self.git(
			&["commit", "-q", "-m", "fixture move"],
			"committer@example.com",
			unix_ts,
		);
	}

	pub(crate) fn head(&self) -> String {
		let output = git::command(self.path())
			.args(["rev-parse", "HEAD"])
			.output()
			.unwrap();
		assert!(output.status.success());
		String::from_utf8(output.stdout).unwrap().trim().to_owned()
	}

	pub(crate) fn bare_clone(&self) -> TempDir {
		let clone = TempDir::new().unwrap();
		let status = git::command(self.path())
			.args(["clone", "-q", "--bare"])
			.arg(self.path())
			.arg(clone.path())
			.status()
			.unwrap();
		assert!(status.success());
		clone
	}

	fn git(&self, args: &[&str], author_email: &str, unix_ts: i64) {
		let date = format!("@{unix_ts} +0000");
		let status = git::command(self.path())
			.args(args)
			.env("GIT_AUTHOR_NAME", "Fixture")
			.env("GIT_AUTHOR_EMAIL", author_email)
			.env("GIT_AUTHOR_DATE", &date)
			.env("GIT_COMMITTER_NAME", "Fixture")
			.env("GIT_COMMITTER_EMAIL", "committer@example.com")
			.env("GIT_COMMITTER_DATE", &date)
			.status()
			.unwrap();
		assert!(status.success());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fixture_supports_delete_move_and_head() {
		let repo = FixtureRepo::new();
		repo.commit(
			"a@example.com",
			1,
			&[("a.c", Some("int a;")), ("b.c", Some("int b;"))],
		);
		let before = repo.head();
		repo.commit("b@example.com", 2, &[("b.c", None)]);
		repo.mv("a.c", "dir/a.c", 3);
		assert_ne!(before, repo.head());
		assert!(repo.path().join("dir/a.c").exists());
		assert!(!repo.path().join("b.c").exists());
	}

	#[test]
	fn head_fails_loudly_without_commits() {
		let repo = FixtureRepo::new();
		let result = std::panic::catch_unwind(|| repo.head());
		assert!(result.is_err());
	}
}
