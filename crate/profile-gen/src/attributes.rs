use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Output, Stdio};

use crate::error::Result;
use crate::git;
use crate::language::{Language, language_by_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
	Default,
	Skip,
	Language(Language),
}

const ATTRIBUTE_NAMES: [&str; 5] = [
	"linguist-vendored",
	"linguist-generated",
	"linguist-documentation",
	"linguist-detectable",
	"linguist-language",
];

pub fn attributes(repo: &Path, paths: &[String]) -> Result<HashMap<String, Attribute>> {
	let map: HashMap<String, Attribute> = paths
		.iter()
		.map(|path| (path.clone(), Attribute::Default))
		.collect();
	if !has_head(repo)? {
		return Ok(map);
	}
	let stdin: Vec<u8> = paths
		.iter()
		.flat_map(|path| path.bytes().chain(std::iter::once(0)))
		.collect();
	run_git(
		repo,
		&["check-attr", "--source", "HEAD", "-z", "--stdin"]
			.into_iter()
			.chain(ATTRIBUTE_NAMES)
			.collect::<Vec<_>>(),
		&stdin,
	)
	.map(|output| apply_output(map, &output))
}

fn apply_output(mut map: HashMap<String, Attribute>, output: &[u8]) -> HashMap<String, Attribute> {
	let fields: Vec<&[u8]> = output.split(|byte| *byte == 0).collect();
	for triple in fields.as_chunks::<3>().0 {
		let path = String::from_utf8_lossy(triple[0]).into_owned();
		let name = String::from_utf8_lossy(triple[1]);
		let value = String::from_utf8_lossy(triple[2]);
		let current = map.get(&path).copied().unwrap_or(Attribute::Default);
		map.insert(path, merge(current, &name, &value));
	}
	map
}

fn merge(current: Attribute, name: &str, value: &str) -> Attribute {
	if current == Attribute::Skip {
		return current;
	}
	let skips = match name {
		"linguist-vendored" | "linguist-generated" | "linguist-documentation" => {
			matches!(value, "set" | "true")
		}
		"linguist-detectable" => matches!(value, "unset" | "false"),
		_ => false,
	};
	if skips {
		return Attribute::Skip;
	}
	if name == "linguist-language" {
		return language_by_name(value).map_or(current, Attribute::Language);
	}
	current
}

pub fn attributes_blob(repo: &Path) -> Result<Option<String>> {
	if !has_head(repo)? {
		return Ok(None);
	}
	run_git(repo, &["ls-tree", "HEAD", ".gitattributes"], &[]).map(|output| {
		String::from_utf8_lossy(&output)
			.split_whitespace()
			.nth(2)
			.map(str::to_owned)
	})
}

fn has_head(repo: &Path) -> Result<bool> {
	let output = git::command(repo)
		.args(["rev-parse", "--verify", "--quiet", "HEAD"])
		.output()?;
	match output.status.code() {
		Some(0) => Ok(true),
		Some(1) => Ok(false),
		_ => Err(git_failure(repo, &["rev-parse"], &output.stderr)),
	}
}

fn git_failure(
	repo: &Path,
	args: &[&str],
	stderr: &[u8],
) -> Box<dyn std::error::Error + Send + Sync> {
	format!(
		"git {} in {}: {}",
		args[0],
		repo.display(),
		String::from_utf8_lossy(stderr).trim()
	)
	.into()
}

fn run_git(repo: &Path, args: &[&str], stdin: &[u8]) -> Result<Vec<u8>> {
	let child = git::command(repo)
		.args(args)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn();
	child
		.and_then(|child| wait_feeding(child, stdin))
		.map_err(Into::into)
		.and_then(|output| {
			if output.status.success() {
				Ok(output.stdout)
			} else {
				Err(git_failure(repo, args, &output.stderr))
			}
		})
}

fn wait_feeding(mut child: Child, stdin: &[u8]) -> std::io::Result<Output> {
	std::thread::scope(|scope| {
		child.stdin.take().into_iter().for_each(|mut child_stdin| {
			scope.spawn(move || child_stdin.write_all(stdin));
		});
		child.wait_with_output()
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fixture::FixtureRepo;

	const GITATTRIBUTES: &str = "libft/** linguist-vendored\ngen/** linguist-generated\ndocs/** linguist-documentation\n*.inc linguist-language=C\nx.rs -linguist-detectable\n";

	fn path_list(list: &[&str]) -> Vec<String> {
		list.iter().map(|path| (*path).to_owned()).collect()
	}

	#[test]
	fn reads_linguist_attributes_from_bare_head() {
		let repo = FixtureRepo::new();
		repo.commit(
			"a@example.com",
			1,
			&[(".gitattributes", Some(GITATTRIBUTES))],
		);
		let bare = repo.bare_clone();
		let paths = path_list(&[
			"libft/a.c",
			"gen/b.c",
			"docs/c.c",
			"x.rs",
			"m.inc",
			"src/d.c",
		]);

		let map = attributes(bare.path(), &paths).unwrap();

		for skipped in ["libft/a.c", "gen/b.c", "docs/c.c", "x.rs"] {
			assert_eq!(map[skipped], Attribute::Skip, "{skipped}");
		}
		assert_eq!(
			map["m.inc"],
			Attribute::Language(language_by_name("C").unwrap())
		);
		assert_eq!(map["src/d.c"], Attribute::Default);
	}

	#[test]
	fn attributes_blob_tracks_gitattributes() {
		let repo = FixtureRepo::new();
		repo.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		assert_eq!(attributes_blob(repo.path()).unwrap(), None);

		repo.commit(
			"a@example.com",
			2,
			&[(".gitattributes", Some("*.c linguist-vendored\n"))],
		);
		let first = attributes_blob(repo.path()).unwrap();
		assert!(first.is_some());

		repo.commit(
			"a@example.com",
			3,
			&[(".gitattributes", Some("*.h linguist-vendored\n"))],
		);
		let second = attributes_blob(repo.path()).unwrap();
		assert!(second.is_some());
		assert_ne!(first, second);
	}

	#[test]
	fn unknown_linguist_language_is_default() {
		let repo = FixtureRepo::new();
		repo.commit(
			"a@example.com",
			1,
			&[(
				".gitattributes",
				Some("*.zz linguist-language=Nonexistent\n"),
			)],
		);

		let map = attributes(repo.path(), &path_list(&["a.zz"])).unwrap();

		assert_eq!(map["a.zz"], Attribute::Default);
	}

	#[test]
	fn explicit_false_does_not_skip() {
		let repo = FixtureRepo::new();
		repo.commit(
			"a@example.com",
			1,
			&[(".gitattributes", Some("*.c linguist-vendored=false\n"))],
		);

		let map = attributes(repo.path(), &path_list(&["a.c"])).unwrap();

		assert_eq!(map["a.c"], Attribute::Default);
	}

	#[test]
	fn true_and_false_spellings_skip() {
		let repo = FixtureRepo::new();
		repo.commit(
			"a@example.com",
			1,
			&[(
				".gitattributes",
				Some("v.c linguist-vendored=true\ng.c linguist-generated=true\nd.c linguist-documentation=true\nn.c linguist-detectable=false\n"),
			)],
		);

		let map = attributes(repo.path(), &path_list(&["v.c", "g.c", "d.c", "n.c"])).unwrap();

		for skipped in ["v.c", "g.c", "d.c", "n.c"] {
			assert_eq!(map[skipped], Attribute::Skip, "{skipped}");
		}
	}

	#[test]
	fn empty_repository_has_no_blob_and_default_attributes() {
		let repo = FixtureRepo::new();
		let bare = repo.bare_clone();
		for path in [repo.path(), bare.path()] {
			assert_eq!(attributes_blob(path).unwrap(), None);
			let map = attributes(path, &path_list(&["a.c"])).unwrap();
			assert_eq!(map["a.c"], Attribute::Default);
		}
	}

	#[test]
	fn git_errors_name_command_and_repo() {
		let dir = tempfile::TempDir::new().unwrap();
		let message = attributes_blob(dir.path()).unwrap_err().to_string();
		assert!(message.starts_with("git "), "{message}");
		assert!(
			message.contains(&dir.path().display().to_string()),
			"{message}"
		);
	}

	#[test]
	fn fails_outside_a_repository() {
		let dir = tempfile::TempDir::new().unwrap();
		assert!(attributes(dir.path(), &path_list(&["a.c"])).is_err());
		assert!(attributes_blob(dir.path()).is_err());
	}

	#[test]
	fn path_outside_repository_is_an_error() {
		let repo = FixtureRepo::new();
		repo.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		let message = attributes(repo.path(), &path_list(&["../outside.c"]))
			.unwrap_err()
			.to_string();
		assert!(message.starts_with("git check-attr in "), "{message}");
	}

	#[test]
	fn fails_on_missing_directory() {
		let missing = std::path::Path::new("/nonexistent/profile-gen-fixture");
		assert!(attributes(missing, &[]).is_err());
	}

	#[test]
	fn no_paths_gives_empty_map() {
		let repo = FixtureRepo::new();
		repo.commit("a@example.com", 1, &[("a.c", Some("int a;"))]);
		assert!(attributes(repo.path(), &[]).unwrap().is_empty());
	}
}
