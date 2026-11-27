use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use twox_hash::XxHash3_64;

use crate::attributes::attributes_blob;
use crate::config::LanguageLists;
use crate::error::Result;
use crate::git;
use crate::history::{Deltas, PathLines, Week, add_deltas, walk};
use crate::language::{Detector, LINGUIST_DATA, language_by_name};
use crate::ownership::BLAME_OPTIONS;

const RULES_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
	pub rules_hash: String,
	pub repos: BTreeMap<String, RepoCache>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCache {
	pub last_sha: String,
	pub attributes_blob: Option<String>,
	#[serde(with = "path_lines_format")]
	pub paths: PathLines,
	#[serde(with = "deltas_format")]
	pub deltas: Deltas,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
	#[serde(with = "deltas_format")]
	pub lines: Deltas,
	#[serde(with = "deltas_format")]
	pub repos: Deltas,
}

#[derive(Deserialize)]
struct StoredHash {
	rules_hash: String,
}

impl Cache {
	fn empty(rules_hash: &str) -> Self {
		Self {
			rules_hash: rules_hash.to_owned(),
			repos: BTreeMap::new(),
		}
	}
}

impl Usage {
	#[must_use]
	pub fn merge(mut self, other: &Self) -> Self {
		add_deltas(&mut self.lines, &other.lines);
		add_deltas(&mut self.repos, &other.repos);
		self
	}
}

mod deltas_format {
	use super::{Deltas, Deserialize, Deserializer, Serializer, by_language, parse_week};
	use std::collections::BTreeMap;

	pub(super) fn serialize<S: Serializer>(
		deltas: &Deltas,
		serializer: S,
	) -> std::result::Result<S::Ok, S::Error> {
		serializer.collect_map(
			deltas
				.iter()
				.map(|(week, bucket)| (week.to_string(), bucket)),
		)
	}

	pub(super) fn deserialize<'de, D: Deserializer<'de>>(
		deserializer: D,
	) -> std::result::Result<Deltas, D::Error> {
		BTreeMap::<String, BTreeMap<String, i64>>::deserialize(deserializer)?
			.into_iter()
			.map(|(week, bucket)| Ok((parse_week(&week)?, by_language(bucket)?)))
			.collect()
	}
}

mod path_lines_format {
	use super::{Deserialize, Deserializer, PathLines, Serializer, by_language};
	use std::collections::BTreeMap;

	pub(super) fn serialize<S: Serializer>(
		paths: &PathLines,
		serializer: S,
	) -> std::result::Result<S::Ok, S::Error> {
		serializer.collect_map(paths)
	}

	pub(super) fn deserialize<'de, D: Deserializer<'de>>(
		deserializer: D,
	) -> std::result::Result<PathLines, D::Error> {
		BTreeMap::<String, BTreeMap<String, u64>>::deserialize(deserializer)?
			.into_iter()
			.map(|(path, bucket)| Ok((path, by_language(bucket)?)))
			.collect()
	}
}

fn by_language<E: serde::de::Error, V>(
	bucket: BTreeMap<String, V>,
) -> std::result::Result<BTreeMap<&'static str, V>, E> {
	bucket
		.into_iter()
		.map(|(name, value)| Ok((parse_language(&name)?, value)))
		.collect()
}

fn parse_week<E: serde::de::Error>(text: &str) -> std::result::Result<Week, E> {
	text.parse().map_err(E::custom)
}

fn parse_language<E: serde::de::Error>(name: &str) -> std::result::Result<&'static str, E> {
	language_by_name(name)
		.map(|language| language.name)
		.ok_or_else(|| E::custom(format!("unknown language {name}")))
}

#[must_use]
pub fn rules_hash(
	authors: &[String],
	exclude_repos: &[String],
	contributed: &[String],
	lists: &LanguageLists,
) -> String {
	hash_rules(
		LINGUIST_DATA,
		&BLAME_OPTIONS,
		authors,
		exclude_repos,
		contributed,
		lists,
	)
}

fn hash_rules(
	linguist_data: &str,
	blame_options: &[&str],
	authors: &[String],
	exclude_repos: &[String],
	contributed: &[String],
	lists: &LanguageLists,
) -> String {
	let mut bytes = Vec::new();
	bytes.extend(RULES_VERSION.to_le_bytes());
	push_text(&mut bytes, linguist_data);
	push_text(&mut bytes, &blame_options.join(" "));
	let ambiguous: Vec<String> = lists
		.ambiguous
		.iter()
		.flat_map(|(key, name)| [key.clone(), name.clone()])
		.collect();
	for list in [
		authors,
		exclude_repos,
		contributed,
		&lists.include,
		&lists.exclude,
		&ambiguous,
	] {
		push_text(&mut bytes, &list.len().to_string());
		for entry in list {
			push_text(&mut bytes, entry);
		}
	}
	format!("{:016x}", XxHash3_64::oneshot(&bytes))
}

fn push_text(bytes: &mut Vec<u8>, text: &str) {
	bytes.extend(text.len().to_le_bytes());
	bytes.extend(text.as_bytes());
}

fn is_on_first_parent_chain(repo: &Path, sha: &str) -> bool {
	git::command(repo)
		.args(["rev-list", "--first-parent", "HEAD"])
		.output()
		.is_ok_and(|output| {
			output.status.success()
				&& String::from_utf8_lossy(&output.stdout)
					.lines()
					.any(|listed| listed == sha)
		})
}

pub fn update(
	cache: &mut Cache,
	name: &str,
	repo: &Path,
	authors: &[String],
	detector: &Detector,
) -> Result<()> {
	let blob = attributes_blob(repo)?;
	let reusable = match cache.repos.remove(name) {
		Some(previous)
			if previous.attributes_blob == blob
				&& is_on_first_parent_chain(repo, &previous.last_sha) =>
		{
			Some(previous)
		}
		_ => None,
	};
	let since = reusable
		.as_ref()
		.map(|previous| (previous.last_sha.as_str(), &previous.paths));
	let walked = walk(repo, since, authors, detector)?;
	if let Some(walked) = walked {
		let mut deltas = reusable.map_or_default(|previous| previous.deltas);
		add_deltas(&mut deltas, &walked.deltas);
		cache.repos.insert(
			name.to_owned(),
			RepoCache {
				last_sha: walked.head,
				attributes_blob: blob,
				paths: walked.paths,
				deltas,
			},
		);
	}
	Ok(())
}

pub fn retain(cache: &mut Cache, names: &BTreeSet<String>) {
	cache.repos.retain(|name, _| names.contains(name));
}

pub fn load(path: &Path, rules_hash: &str) -> Result<Cache> {
	match std::fs::read(path) {
		Ok(bytes) => {
			let stored: StoredHash = serde_json::from_slice(&bytes)?;
			if stored.rules_hash == rules_hash {
				Ok(serde_json::from_slice(&bytes)?)
			} else {
				Ok(Cache::empty(rules_hash))
			}
		}
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Cache::empty(rules_hash)),
		Err(error) => Err(error.into()),
	}
}

pub fn save(cache: &Cache, path: &Path) -> Result<()> {
	let mut temporary = PathBuf::from(path);
	temporary.as_mut_os_string().push(".tmp");
	serde_json::to_vec_pretty(cache)
		.map_err(Into::into)
		.and_then(|bytes| std::fs::write(&temporary, bytes).map_err(Into::into))
		.and_then(|()| std::fs::rename(&temporary, path).map_err(Into::into))
}

#[must_use]
pub fn usage(cache: &Cache) -> Usage {
	let mut usage = Usage::default();
	for repo in cache.repos.values() {
		add_deltas(&mut usage.lines, &repo.deltas);
		add_deltas(&mut usage.repos, &presence_changes(&repo.deltas));
	}
	usage
}

fn presence_changes(deltas: &Deltas) -> Deltas {
	let mut level: BTreeMap<&'static str, i64> = BTreeMap::new();
	deltas
		.iter()
		.map(|(week, bucket)| {
			let changes = bucket
				.iter()
				.filter_map(|(language, delta)| {
					let before = level.get(language).copied().unwrap_or(0);
					let after = before + delta;
					level.insert(language, after);
					match (before > 0, after > 0) {
						(false, true) => Some((*language, 1)),
						(true, false) => Some((*language, -1)),
						_ => None,
					}
				})
				.collect();
			(*week, changes)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fixture::{FixtureRepo, shipped_detector};
	use jiff::civil::Date;
	use std::collections::BTreeMap;

	const WEEK_ONE_TS: i64 = 1_795_392_000;
	const WEEK_SECONDS: i64 = 7 * 24 * 3600;
	const ME: &str = "me@x";

	fn code(tag: &str, count: usize) -> String {
		(0..count)
			.map(|index| format!("int {tag}_value_{index}_alpha = compute_value({index}, 2, 3);\n"))
			.collect::<Vec<_>>()
			.concat()
	}

	fn me() -> Vec<String> {
		vec![ME.to_owned()]
	}

	fn week(offset: i64) -> Week {
		Date::constant(2026, 11, 23).saturating_add(jiff::Span::new().weeks(offset))
	}

	fn deltas(entries: &[(i64, &[(&'static str, i64)])]) -> Deltas {
		entries
			.iter()
			.map(|(offset, bucket)| (week(*offset), bucket.iter().copied().collect()))
			.collect()
	}

	fn fresh(repo: &FixtureRepo) -> RepoCache {
		let walked = walk(repo.path(), None, &me(), &shipped_detector())
			.unwrap()
			.unwrap();
		RepoCache {
			last_sha: walked.head,
			attributes_blob: attributes_blob(repo.path()).unwrap(),
			paths: walked.paths,
			deltas: walked.deltas,
		}
	}

	fn cached(repo: &FixtureRepo) -> Cache {
		let mut cache = Cache::empty("h");
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();
		cache
	}

	#[test]
	fn update_appends_only_new_commits() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		let mut cache = cached(&repo);
		let first = repo.head();
		assert_eq!(cache.repos["r"].last_sha, first);

		repo.commit(
			ME,
			WEEK_ONE_TS + WEEK_SECONDS,
			&[("b.rs", Some(&code("b", 3)))],
		);
		repo.commit(ME, WEEK_ONE_TS + 1, &[("a.rs", Some(&code("a", 4)))]);
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"], fresh(&repo));
		assert_eq!(
			cache.repos["r"].deltas,
			deltas(&[(0, &[("Rust", 10)]), (1, &[("Rust", -3)])])
		);
		assert_ne!(cache.repos["r"].last_sha, first);
	}

	#[test]
	fn update_without_new_commits_keeps_the_repo() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		let mut cache = cached(&repo);
		let before = cache.clone();

		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache, before);
	}

	#[test]
	fn rewritten_history_recomputes() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		repo.commit(ME, WEEK_ONE_TS + 1, &[("b.rs", Some(&code("b", 5)))]);
		let mut cache = cached(&repo);

		repo.amend(ME, WEEK_ONE_TS + 2, &[("b.rs", &code("b", 8))]);
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"], fresh(&repo));
		assert_eq!(cache.repos["r"].deltas, deltas(&[(0, &[("Rust", 18)])]));
	}

	#[test]
	fn incremental_after_fast_forward_onto_a_merge_of_main_equals_full_walk() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.c", Some(&code("a", 4)))]);
		repo.branch("feature");
		repo.switch("main");
		repo.commit(ME, WEEK_ONE_TS + 1, &[("b.c", Some(&code("b", 3)))]);
		let mut cache = cached(&repo);
		repo.switch("feature");
		repo.commit(
			ME,
			WEEK_ONE_TS + WEEK_SECONDS,
			&[("c.rs", Some(&code("c", 2)))],
		);
		repo.merge(ME, WEEK_ONE_TS + WEEK_SECONDS + 1, "main", &[]);
		repo.switch("main");
		repo.fast_forward("feature");

		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"], fresh(&repo));
		assert_eq!(
			cache.repos["r"].deltas,
			deltas(&[(0, &[("C", 4)]), (1, &[("C", 3), ("Rust", 2)])])
		);
	}

	#[test]
	fn unknown_last_sha_recomputes() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		let mut cache = cached(&repo);
		cache.repos.get_mut("r").unwrap().last_sha = "0".repeat(40);

		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"], fresh(&repo));
	}

	#[test]
	fn attributes_change_recomputes() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("libft/a.c", Some(&code("a", 10)))]);
		repo.commit(ME, WEEK_ONE_TS + 1, &[("b.rs", Some(&code("b", 1)))]);
		let mut cache = cached(&repo);
		assert_eq!(
			cache.repos["r"].deltas,
			deltas(&[(0, &[("C", 10), ("Rust", 1)])])
		);

		repo.commit(
			ME,
			WEEK_ONE_TS + 2,
			&[(".gitattributes", Some("libft/** linguist-vendored\n"))],
		);
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"].deltas, deltas(&[(0, &[("Rust", 1)])]));
		assert_eq!(cache.repos["r"], fresh(&repo));
		assert!(cache.repos["r"].attributes_blob.is_some());
	}

	#[test]
	fn empty_repo_has_no_entry() {
		let repo = FixtureRepo::new();
		let mut cache = Cache::empty("h");
		cache.repos.insert(
			"r".to_owned(),
			RepoCache {
				last_sha: "abc".to_owned(),
				..RepoCache::default()
			},
		);

		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert!(cache.repos.is_empty());
	}

	#[test]
	fn update_fails_outside_a_repository() {
		let dir = tempfile::TempDir::new().unwrap();
		let mut cache = Cache::empty("h");
		assert!(update(&mut cache, "r", dir.path(), &me(), &shipped_detector()).is_err());
	}

	fn no_lists() -> LanguageLists {
		LanguageLists::default()
	}

	#[test]
	fn rules_hash_depends_on_every_input() {
		let hash = |authors: &[String], exclude_repos: &[String]| {
			rules_hash(authors, exclude_repos, &[], &no_lists())
		};
		let base = hash(&me(), &["x".to_owned()]);
		assert_eq!(base, hash(&me(), &["x".to_owned()]));
		assert_ne!(base, hash(&["you@x".to_owned()], &["x".to_owned()]));
		assert_ne!(base, hash(&me(), &["y".to_owned()]));
		assert_ne!(base, hash(&[], &[]));
		assert_ne!(
			hash(&["a".to_owned(), "b".to_owned()], &[]),
			hash(&["a".to_owned()], &["b".to_owned()])
		);
		assert_eq!(base.len(), 16);
	}

	#[test]
	fn rules_hash_changes_with_linguist_data_blame_options_or_lists() {
		let names =
			|list: &[&str]| -> Vec<String> { list.iter().map(|name| (*name).to_owned()).collect() };
		let ambiguous = |name: &str| BTreeMap::from([(".h".to_owned(), name.to_owned())]);
		let base = rules_hash(&me(), &[], &[], &no_lists());
		assert_eq!(
			base,
			hash_rules(LINGUIST_DATA, &BLAME_OPTIONS, &me(), &[], &[], &no_lists())
		);
		assert_ne!(
			base,
			hash_rules("{}", &BLAME_OPTIONS, &me(), &[], &[], &no_lists())
		);
		assert_ne!(
			base,
			hash_rules(LINGUIST_DATA, &["-w"], &me(), &[], &[], &no_lists())
		);
		let variants = [
			LanguageLists {
				include: names(&["JSON"]),
				..no_lists()
			},
			LanguageLists {
				exclude: names(&["JSON"]),
				..no_lists()
			},
			LanguageLists {
				include: names(&["JSON", "TOML"]),
				..no_lists()
			},
			LanguageLists {
				include: names(&["JSON"]),
				exclude: names(&["TOML"]),
				..no_lists()
			},
			LanguageLists {
				ambiguous: ambiguous("C"),
				..no_lists()
			},
			LanguageLists {
				ambiguous: ambiguous("C++"),
				..no_lists()
			},
		];
		let mut hashes: Vec<String> = variants
			.iter()
			.map(|lists| rules_hash(&me(), &[], &[], lists))
			.chain([base])
			.collect();
		let count = hashes.len();
		hashes.sort();
		hashes.dedup();
		assert_eq!(hashes.len(), count);
	}

	#[test]
	fn rules_hash_covers_contributed() {
		let hash = |exclude_repos: &[&str], contributed: &[&str]| {
			let owned = |list: &[&str]| -> Vec<String> {
				list.iter().map(|entry| (*entry).to_owned()).collect()
			};
			rules_hash(
				&me(),
				&owned(exclude_repos),
				&owned(contributed),
				&no_lists(),
			)
		};
		let base = hash(&[], &["f/a"]);
		assert_eq!(base, hash(&[], &["f/a"]));
		assert_ne!(base, hash(&[], &[]));
		assert_ne!(base, hash(&[], &["f/b"]));
		assert_ne!(base, hash(&["f/a"], &[]));
	}

	#[test]
	fn load_resets_on_rules_hash_mismatch() {
		let dir = tempfile::TempDir::new().unwrap();
		let path = dir.path().join("cache.json");
		let mut cache = Cache::empty("old");
		cache.repos.insert("r".to_owned(), RepoCache::default());
		save(&cache, &path).unwrap();

		assert_eq!(load(&path, "old").unwrap(), cache);
		assert_eq!(load(&path, "new").unwrap(), Cache::empty("new"));
	}

	#[test]
	fn different_blame_options_give_a_different_hash() {
		let dir = tempfile::TempDir::new().unwrap();
		let path = dir.path().join("cache.json");
		let added_lines_hash = hash_rules(LINGUIST_DATA, &[], &me(), &[], &[], &no_lists());
		let old_format = format!(
			r#"{{"rules_hash":"{added_lines_hash}","repos":{{"r":{{"last_sha":"a","attributes_blob":null,"weeks":{{"2026-11-23":{{"Rust":1}}}}}}}}}}"#
		);
		std::fs::write(&path, old_format).unwrap();
		let current = rules_hash(&me(), &[], &[], &no_lists());

		assert_ne!(current, added_lines_hash);
		assert_eq!(load(&path, &current).unwrap(), Cache::empty(&current));
		assert!(load(&path, &added_lines_hash).is_err());
	}

	#[test]
	fn load_resets_stale_file_with_unknown_language() {
		let dir = tempfile::TempDir::new().unwrap();
		let path = dir.path().join("cache.json");
		let stale = r#"{"rules_hash":"old","repos":{"r":{"last_sha":"a","attributes_blob":null,"paths":{},"deltas":{"2026-11-23":{"Removed":1}}}}}"#;
		std::fs::write(&path, stale).unwrap();

		assert_eq!(load(&path, "new").unwrap(), Cache::empty("new"));
		assert!(load(&path, "old").is_err());
	}

	#[test]
	fn load_missing_file_is_empty() {
		let dir = tempfile::TempDir::new().unwrap();
		let loaded = load(&dir.path().join("none.json"), "h").unwrap();
		assert_eq!(loaded, Cache::empty("h"));
	}

	#[test]
	fn load_rejects_unreadable_and_malformed_files() {
		let dir = tempfile::TempDir::new().unwrap();
		assert!(load(dir.path(), "h").is_err());

		let path = dir.path().join("cache.json");
		let bad_documents = [
			"not json",
			r#"{"rules_hash":"h","repos":{"r":{"last_sha":"a","attributes_blob":null,"paths":{},"deltas":{"2026-11-23":{"Nope":1}}}}}"#,
			r#"{"rules_hash":"h","repos":{"r":{"last_sha":"a","attributes_blob":null,"paths":{},"deltas":{"not-a-date":{"Rust":1}}}}}"#,
			r#"{"rules_hash":"h","repos":{"r":{"last_sha":"a","attributes_blob":null,"paths":{"a.rs":{"Nope":1}},"deltas":{}}}}"#,
			r#"{"rules_hash":"h","repos":{"r":{"last_sha":"a","attributes_blob":null,"paths":{"a.rs":{"Rust":-1}},"deltas":{}}}}"#,
		];
		for document in bad_documents {
			std::fs::write(&path, document).unwrap();
			assert!(load(&path, "h").is_err(), "{document}");
		}
	}

	#[test]
	fn save_is_deterministic_and_leaves_no_temp_file() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		let cache = cached(&repo);
		let dir = tempfile::TempDir::new().unwrap();
		let first = dir.path().join("a.json");
		let second = dir.path().join("b.json");

		save(&cache, &first).unwrap();
		save(&cache, &second).unwrap();

		assert_eq!(
			std::fs::read(&first).unwrap(),
			std::fs::read(&second).unwrap()
		);
		assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
	}

	#[test]
	fn save_fails_in_missing_directory() {
		let missing = Path::new("/nonexistent/profile-gen-cache/cache.json");
		assert!(save(&Cache::empty("h"), missing).is_err());
	}

	#[test]
	fn retain_drops_deleted_repos() {
		let mut cache = Cache::empty("h");
		for name in ["a", "b", "c"] {
			cache.repos.insert(name.to_owned(), RepoCache::default());
		}
		let kept = BTreeSet::from(["a".to_owned(), "c".to_owned()]);

		retain(&mut cache, &kept);

		assert_eq!(
			cache.repos.keys().map(String::as_str).collect::<Vec<_>>(),
			["a", "c"]
		);
	}

	#[test]
	fn roundtrip_json() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		repo.commit(
			ME,
			WEEK_ONE_TS + WEEK_SECONDS,
			&[("b.c", Some(&code("b", 2)))],
		);
		let cache = cached(&repo);

		let json = serde_json::to_string(&cache).unwrap();
		assert!(
			json.contains(r#""paths":{"a.rs":{"Rust":10},"b.c":{"C":2}}"#),
			"{json}"
		);
		assert!(
			json.contains(r#""deltas":{"2026-11-23":{"Rust":10},"2026-11-30":{"C":2}}"#),
			"{json}"
		);
		assert_eq!(serde_json::from_str::<Cache>(&json).unwrap(), cache);

		let usage = usage(&cache);
		let json = serde_json::to_string(&usage).unwrap();
		assert_eq!(
			json,
			r#"{"lines":{"2026-11-23":{"Rust":10},"2026-11-30":{"C":2}},"repos":{"2026-11-23":{"Rust":1},"2026-11-30":{"C":1}}}"#
		);
		assert_eq!(serde_json::from_str::<Usage>(&json).unwrap(), usage);
	}

	#[test]
	fn usage_of_the_wrong_shape_is_rejected() {
		let rejected = [
			r#"{"lines":5,"repos":{}}"#,
			r#"{"lines":{},"repos":5}"#,
			r#"{"lines":{},"repos":{"2026-11-23":{"Nope":1}}}"#,
			r#"{"lines":{},"repos":{"2026-13-40":{"Rust":1}}}"#,
			r#"{"weeks":{},"first_use":{}}"#,
		];
		for document in rejected {
			assert!(
				serde_json::from_str::<Usage>(document).is_err(),
				"{document}"
			);
		}
	}

	#[test]
	fn update_fails_on_corrupt_history() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&code("a", 10)))]);
		let blob = git::command(repo.path())
			.args(["hash-object", "a.rs"])
			.output()
			.unwrap()
			.stdout;
		let blob = String::from_utf8(blob).unwrap();
		let (fan_out, rest) = blob.trim().split_at(2);
		std::fs::remove_file(repo.path().join(".git/objects").join(fan_out).join(rest)).unwrap();
		let mut cache = Cache::empty("h");

		assert!(update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).is_err());
	}

	fn repo_cache(entries: &[(i64, &[(&'static str, i64)])]) -> RepoCache {
		RepoCache {
			deltas: deltas(entries),
			..RepoCache::default()
		}
	}

	#[test]
	fn usage_repo_count_rises_and_falls_with_level() {
		let mut cache = Cache::empty("h");
		cache.repos.insert(
			"one".to_owned(),
			repo_cache(&[
				(0, &[("Rust", 5)]),
				(1, &[("C", 3), ("Rust", -5)]),
				(2, &[("Rust", 2)]),
				(4, &[("Rust", 1), ("C", -1)]),
			]),
		);
		cache
			.repos
			.insert("two".to_owned(), repo_cache(&[(3, &[("Rust", 4)])]));

		let usage = usage(&cache);

		assert_eq!(
			usage.lines,
			deltas(&[
				(0, &[("Rust", 5)]),
				(1, &[("C", 3), ("Rust", -5)]),
				(2, &[("Rust", 2)]),
				(3, &[("Rust", 4)]),
				(4, &[("C", -1), ("Rust", 1)]),
			])
		);
		assert_eq!(
			usage.repos,
			deltas(&[
				(0, &[("Rust", 1)]),
				(1, &[("C", 1), ("Rust", -1)]),
				(2, &[("Rust", 1)]),
				(3, &[("Rust", 1)]),
			])
		);
	}

	#[test]
	fn merge_adds_lines_and_repos() {
		let left = Usage {
			lines: deltas(&[(0, &[("Rust", 5)]), (1, &[("C", 4)])]),
			repos: deltas(&[(0, &[("Rust", 1)]), (1, &[("C", 1)])]),
		};
		let right = Usage {
			lines: deltas(&[(1, &[("C", -4), ("Rust", 2)])]),
			repos: deltas(&[(1, &[("C", -1), ("Rust", 1)])]),
		};

		let merged = left.merge(&right);

		assert_eq!(
			merged,
			Usage {
				lines: deltas(&[(0, &[("Rust", 5)]), (1, &[("Rust", 2)])]),
				repos: deltas(&[(0, &[("Rust", 1)]), (1, &[("Rust", 1)])]),
			}
		);
	}
}
