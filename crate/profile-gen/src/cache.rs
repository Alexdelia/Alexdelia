use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use twox_hash::XxHash3_64;

use crate::attributes::attributes_blob;
use crate::config::LanguageLists;
use crate::error::Result;
use crate::git;
use crate::history::{Week, Weeks, weekly_added};
use crate::language::{Detector, LINGUIST_DATA, language_by_name};

const RULES_VERSION: u32 = 2;

type LanguageLines = BTreeMap<&'static str, u64>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
	pub rules_hash: String,
	pub repos: BTreeMap<String, RepoCache>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoCache {
	pub last_sha: String,
	pub attributes_blob: Option<String>,
	#[serde(with = "weeks_format")]
	pub weeks: Weeks,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
	#[serde(with = "weeks_format")]
	pub weeks: Weeks,
	#[serde(with = "first_use_format")]
	pub first_use: BTreeMap<&'static str, Vec<Week>>,
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
	pub fn merge(mut self, other: Self) -> Self {
		add_weeks(&mut self.weeks, &other.weeks);
		for (language, mut list) in other.first_use {
			let merged = self.first_use.entry(language).or_default();
			merged.append(&mut list);
			merged.sort_unstable();
		}
		self
	}
}

mod weeks_format {
	use super::{
		BTreeMap, Deserialize, Deserializer, LanguageLines, Serializer, Weeks, parse_language,
		parse_week,
	};

	pub(super) fn serialize<S: Serializer>(
		weeks: &Weeks,
		serializer: S,
	) -> std::result::Result<S::Ok, S::Error> {
		serializer.collect_map(weeks.iter().map(|(week, lines)| (week.to_string(), lines)))
	}

	pub(super) fn deserialize<'de, D: Deserializer<'de>>(
		deserializer: D,
	) -> std::result::Result<Weeks, D::Error> {
		BTreeMap::<String, BTreeMap<String, u64>>::deserialize(deserializer)?
			.into_iter()
			.map(|(week, lines)| Ok((parse_week(&week)?, parse_lines(lines)?)))
			.collect()
	}

	fn parse_lines<E: serde::de::Error>(
		lines: BTreeMap<String, u64>,
	) -> std::result::Result<LanguageLines, E> {
		lines
			.into_iter()
			.map(|(name, count)| Ok((parse_language(&name)?, count)))
			.collect()
	}
}

mod first_use_format {
	use super::{
		BTreeMap, Deserialize, Deserializer, Serializer, Week, parse_language, parse_week,
	};

	pub(super) fn serialize<S: Serializer>(
		first_use: &BTreeMap<&'static str, Vec<Week>>,
		serializer: S,
	) -> std::result::Result<S::Ok, S::Error> {
		serializer.collect_map(first_use.iter().map(|(language, list)| {
			(
				language,
				list.iter().map(ToString::to_string).collect::<Vec<_>>(),
			)
		}))
	}

	pub(super) fn deserialize<'de, D: Deserializer<'de>>(
		deserializer: D,
	) -> std::result::Result<BTreeMap<&'static str, Vec<Week>>, D::Error> {
		BTreeMap::<String, Vec<String>>::deserialize(deserializer)?
			.into_iter()
			.map(|(name, list)| {
				let weeks = list
					.iter()
					.map(|week| parse_week(week))
					.collect::<std::result::Result<_, _>>()?;
				Ok((parse_language(&name)?, weeks))
			})
			.collect()
	}
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
pub fn rules_hash(authors: &[String], exclude_repos: &[String], lists: &LanguageLists) -> String {
	hash_rules(LINGUIST_DATA, authors, exclude_repos, lists)
}

fn hash_rules(
	linguist_data: &str,
	authors: &[String],
	exclude_repos: &[String],
	lists: &LanguageLists,
) -> String {
	let mut bytes = Vec::new();
	bytes.extend(RULES_VERSION.to_le_bytes());
	push_text(&mut bytes, linguist_data);
	let ambiguous: Vec<String> = lists
		.ambiguous
		.iter()
		.flat_map(|(key, name)| [key.clone(), name.clone()])
		.collect();
	for list in [
		authors,
		exclude_repos,
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

fn add_weeks(target: &mut Weeks, source: &Weeks) {
	for (week, lines) in source {
		let bucket = target.entry(*week).or_default();
		for (language, count) in lines {
			*bucket.entry(language).or_default() += count;
		}
	}
}

fn is_ancestor_of_head(repo: &Path, sha: &str) -> bool {
	git::command(repo)
		.args(["merge-base", "--is-ancestor", sha, "HEAD"])
		.output()
		.is_ok_and(|output| output.status.success())
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
				&& is_ancestor_of_head(repo, &previous.last_sha) =>
		{
			Some(previous)
		}
		_ => None,
	};
	let since = reusable.as_ref().map(|previous| previous.last_sha.as_str());
	let (added, head) = weekly_added(repo, since, authors, detector)?;
	let mut weeks = reusable.map_or_default(|previous| previous.weeks);
	add_weeks(&mut weeks, &added);
	if let Some(last_sha) = head {
		cache.repos.insert(
			name.to_owned(),
			RepoCache {
				last_sha,
				attributes_blob: blob,
				weeks,
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
		add_weeks(&mut usage.weeks, &repo.weeks);
		let mut seen = BTreeSet::new();
		for (week, lines) in &repo.weeks {
			for (language, count) in lines {
				if *count > 0 && seen.insert(*language) {
					usage.first_use.entry(language).or_default().push(*week);
				}
			}
		}
	}
	for list in usage.first_use.values_mut() {
		list.sort_unstable();
	}
	usage
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

	fn lines(count: usize) -> String {
		"x\n".repeat(count)
	}

	fn me() -> Vec<String> {
		vec![ME.to_owned()]
	}

	fn week(offset: i64) -> Week {
		Date::constant(2026, 11, 23).saturating_add(jiff::Span::new().weeks(offset))
	}

	fn bucket(entries: &[(&'static str, u64)]) -> BTreeMap<&'static str, u64> {
		entries.iter().copied().collect()
	}

	fn fresh_weeks(repo: &FixtureRepo) -> Weeks {
		weekly_added(repo.path(), None, &me(), &shipped_detector())
			.unwrap()
			.0
	}

	fn cached(repo: &FixtureRepo) -> Cache {
		let mut cache = Cache::empty("h");
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();
		cache
	}

	#[test]
	fn update_appends_only_new_commits() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		let mut cache = cached(&repo);
		let first = repo.head();
		assert_eq!(cache.repos["r"].last_sha, first);

		repo.commit(ME, WEEK_ONE_TS + WEEK_SECONDS, &[("b.rs", Some(&lines(3)))]);
		repo.commit(ME, WEEK_ONE_TS + 1, &[("c.c", Some(&lines(2)))]);
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"].weeks, fresh_weeks(&repo));
		assert_eq!(cache.repos["r"].last_sha, repo.head());
		assert_ne!(cache.repos["r"].last_sha, first);
	}

	#[test]
	fn update_without_new_commits_keeps_weeks() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		let mut cache = cached(&repo);
		let before = cache.clone();

		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache, before);
	}

	#[test]
	fn rewritten_history_recomputes() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		repo.commit(ME, WEEK_ONE_TS + 1, &[("b.rs", Some(&lines(5)))]);
		let mut cache = cached(&repo);

		repo.amend(ME, WEEK_ONE_TS + 2, &[("b.rs", &lines(8))]);
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"].weeks, fresh_weeks(&repo));
		assert_eq!(
			cache.repos["r"].weeks,
			Weeks::from([(week(0), bucket(&[("Rust", 18)]))])
		);
		assert_eq!(cache.repos["r"].last_sha, repo.head());
	}

	#[test]
	fn unknown_last_sha_recomputes() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		let mut cache = cached(&repo);
		cache.repos.get_mut("r").unwrap().last_sha = "0".repeat(40);

		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"].weeks, fresh_weeks(&repo));
		assert_eq!(cache.repos["r"].last_sha, repo.head());
	}

	#[test]
	fn attributes_change_recomputes() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("libft/a.c", Some(&lines(10)))]);
		repo.commit(ME, WEEK_ONE_TS + 1, &[("b.rs", Some(&lines(1)))]);
		let mut cache = cached(&repo);
		assert_eq!(
			cache.repos["r"].weeks[&week(0)],
			bucket(&[("C", 10), ("Rust", 1)])
		);

		repo.commit(
			ME,
			WEEK_ONE_TS + 2,
			&[(".gitattributes", Some("libft/** linguist-vendored\n"))],
		);
		update(&mut cache, "r", repo.path(), &me(), &shipped_detector()).unwrap();

		assert_eq!(cache.repos["r"].weeks[&week(0)], bucket(&[("Rust", 1)]));
		assert_eq!(cache.repos["r"].weeks, fresh_weeks(&repo));
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
				attributes_blob: None,
				weeks: Weeks::new(),
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
			rules_hash(authors, exclude_repos, &no_lists())
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
	fn rules_hash_changes_with_linguist_data_or_lists() {
		let names =
			|list: &[&str]| -> Vec<String> { list.iter().map(|name| (*name).to_owned()).collect() };
		let ambiguous = |name: &str| BTreeMap::from([(".h".to_owned(), name.to_owned())]);
		let base = rules_hash(&me(), &[], &no_lists());
		assert_eq!(base, hash_rules(LINGUIST_DATA, &me(), &[], &no_lists()));
		assert_ne!(base, hash_rules("{}", &me(), &[], &no_lists()));
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
			.map(|lists| rules_hash(&me(), &[], lists))
			.chain([base])
			.collect();
		let count = hashes.len();
		hashes.sort();
		hashes.dedup();
		assert_eq!(hashes.len(), count);
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
	fn load_resets_stale_file_with_unknown_language() {
		let dir = tempfile::TempDir::new().unwrap();
		let path = dir.path().join("cache.json");
		let stale = r#"{"rules_hash":"old","repos":{"r":{"last_sha":"a","attributes_blob":null,"weeks":{"2026-11-23":{"Removed":1}}}}}"#;
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
			r#"{"rules_hash":"h","repos":{"r":{"last_sha":"a","attributes_blob":null,"weeks":{"2026-11-23":{"Nope":1}}}}}"#,
			r#"{"rules_hash":"h","repos":{"r":{"last_sha":"a","attributes_blob":null,"weeks":{"not-a-date":{"Rust":1}}}}}"#,
		];
		for document in bad_documents {
			std::fs::write(&path, document).unwrap();
			assert!(load(&path, "h").is_err(), "{document}");
		}
	}

	#[test]
	fn save_is_deterministic_and_leaves_no_temp_file() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
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
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
		repo.commit(ME, WEEK_ONE_TS + WEEK_SECONDS, &[("b.c", Some(&lines(2)))]);
		let cache = cached(&repo);

		let json = serde_json::to_string(&cache).unwrap();
		assert!(json.contains("\"2026-11-23\""));
		assert!(json.contains("\"Rust\""));
		assert_eq!(serde_json::from_str::<Cache>(&json).unwrap(), cache);

		let usage = usage(&cache);
		let json = serde_json::to_string(&usage).unwrap();
		assert_eq!(serde_json::from_str::<Usage>(&json).unwrap(), usage);
	}

	#[test]
	fn weeks_of_the_wrong_shape_are_rejected() {
		assert!(serde_json::from_str::<Usage>(r#"{"weeks":5,"first_use":{}}"#).is_err());
		assert!(serde_json::from_str::<Usage>(r#"{"weeks":{},"first_use":5}"#).is_err());
	}

	#[test]
	fn update_fails_on_corrupt_history() {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[("a.rs", Some(&lines(10)))]);
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

	#[test]
	fn usage_rejects_malformed_first_use() {
		let unknown_language = r#"{"weeks":{},"first_use":{"Nope":["2026-11-23"]}}"#;
		let bad_week = r#"{"weeks":{},"first_use":{"Rust":["2026-13-40"]}}"#;
		assert!(serde_json::from_str::<Usage>(unknown_language).is_err());
		assert!(serde_json::from_str::<Usage>(bad_week).is_err());
	}

	fn repo_cache(entries: &[(Week, &[(&'static str, u64)])]) -> RepoCache {
		RepoCache {
			last_sha: String::new(),
			attributes_blob: None,
			weeks: entries
				.iter()
				.map(|(week, list)| (*week, bucket(list)))
				.collect(),
		}
	}

	fn two_repo_cache() -> Cache {
		let mut cache = Cache::empty("h");
		cache.repos.insert(
			"one".to_owned(),
			repo_cache(&[
				(week(0), &[("Rust", 5)]),
				(week(1), &[("C", 4), ("Rust", 1)]),
			]),
		);
		cache.repos.insert(
			"two".to_owned(),
			repo_cache(&[(week(2), &[("Rust", 7), ("Nix", 0)])]),
		);
		cache
	}

	#[test]
	fn usage_sums_weeks_and_lists_first_use_per_repo() {
		let usage = usage(&two_repo_cache());

		assert_eq!(usage.first_use["Rust"], [week(0), week(2)]);
		assert_eq!(usage.first_use["C"], [week(1)]);
		assert!(!usage.first_use.contains_key("Nix"));
		assert_eq!(
			usage.weeks,
			Weeks::from([
				(week(0), bucket(&[("Rust", 5)])),
				(week(1), bucket(&[("C", 4), ("Rust", 1)])),
				(week(2), bucket(&[("Nix", 0), ("Rust", 7)])),
			])
		);
	}

	#[test]
	fn usage_merge_concatenates_sorted() {
		let left = usage(&two_repo_cache());
		let mut other = Cache::empty("h");
		other
			.repos
			.insert("three".to_owned(), repo_cache(&[(week(1), &[("Rust", 2)])]));
		let right = usage(&other);

		let merged = left.merge(right);

		assert_eq!(merged.first_use["Rust"], [week(0), week(1), week(2)]);
		assert_eq!(merged.first_use["C"], [week(1)]);
		assert_eq!(merged.weeks[&week(1)], bucket(&[("C", 4), ("Rust", 3)]));
	}
}
