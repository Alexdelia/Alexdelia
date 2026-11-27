use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::cache::{self, Usage};
use crate::config::Config;
use crate::error::Result;
use crate::github::{Fetch, RepoInfo, list_contributed, list_public, sync_clone};
use crate::language::Detector;
use crate::render::languages::render_languages;
use crate::theme::Variant;
use crate::timeline::timeline;

const CACHE_DIR: &str = "cache";
const CACHE_FILE: &str = "languages.json";

#[derive(Debug, Clone)]
pub struct Paths {
	pub out: PathBuf,
	pub work: PathBuf,
	pub private: PathBuf,
}

fn read_private(path: &Path) -> Result<Usage> {
	match std::fs::read(path) {
		Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Usage::default()),
		Err(error) => Err(error.into()),
	}
}

fn counted_repos(owned: Vec<RepoInfo>, contributed: Vec<RepoInfo>) -> Vec<RepoInfo> {
	let mut seen: BTreeSet<String> = owned.iter().map(RepoInfo::full_name).collect();
	let not_seen = contributed
		.into_iter()
		.filter(|repo| seen.insert(repo.full_name()));
	owned.into_iter().chain(not_seen).collect()
}

fn refresh_cache(
	config: &Config,
	detector: &Detector,
	fetch: &impl Fetch,
	token: Option<&str>,
	work: &Path,
	cache_path: &Path,
) -> Result<cache::Cache> {
	let rules_hash = cache::rules_hash(
		&config.authors,
		&config.exclude_repos,
		&config.contributed,
		&config.languages,
	);
	let mut cache = cache::load(cache_path, &rules_hash)?;
	let owned = list_public(fetch, &config.owner, token, &config.exclude_repos)?;
	let contributed = list_contributed(fetch, token, &config.contributed)?;
	let repos = counted_repos(owned, contributed);
	for repo in &repos {
		let clone = sync_clone(repo, work)?;
		cache::update(
			&mut cache,
			&repo.full_name(),
			&clone,
			&config.authors,
			detector,
		)?;
	}
	let names: BTreeSet<String> = repos.iter().map(RepoInfo::full_name).collect();
	cache::retain(&mut cache, &names);
	Ok(cache)
}

pub fn render(
	config: &Config,
	fetch: &impl Fetch,
	token: Option<&str>,
	paths: &Paths,
) -> Result<()> {
	let detector = Detector::new(&config.languages)?;
	let private = read_private(&paths.private)?;
	let cache_dir = paths.out.join(CACHE_DIR);
	std::fs::create_dir_all(&cache_dir)?;
	let cache_path = cache_dir.join(CACHE_FILE);
	let cache = refresh_cache(config, &detector, fetch, token, &paths.work, &cache_path)?;
	cache::save(&cache, &cache_path)?;

	let usage = cache::usage(&cache).merge(&private);
	let timeline = timeline(&usage, config.top_n, config.max_frames);
	for (variant, file) in [
		(Variant::Dark, "languages-dark.svg"),
		(Variant::Light, "languages-light.svg"),
	] {
		std::fs::write(
			paths.out.join(file),
			render_languages(&timeline, variant, config.seconds_per_year),
		)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cache::Cache;
	use crate::fixture::{FixtureRepo, shipped_lists};
	use crate::git;
	use crate::github::{NotFound, RepoInfo};
	use std::collections::BTreeMap;
	use tempfile::TempDir;

	const ME: &str = "me@example.com";
	const WEEK_ONE_TS: i64 = 1_577_836_800;
	const WEEK_TWO_TS: i64 = 1_578_441_600;

	const OWNER: &str = "me";

	struct Listing {
		owned: String,
		contributed: BTreeMap<String, String>,
	}

	impl Fetch for Listing {
		fn get(&self, url: &str, _token: Option<&str>) -> Result<String> {
			if let Some(full_name) = url.strip_prefix("https://api.github.com/repos/") {
				return self
					.contributed
					.get(full_name)
					.cloned()
					.ok_or_else(|| NotFound.into());
			}
			Ok(if url.ends_with("page=1") {
				self.owned.clone()
			} else {
				"[]".to_owned()
			})
		}
	}

	fn repo_json(owner: &str, name: &str, repo: &FixtureRepo) -> String {
		format!(
			r#"{{"owner":{{"login":"{owner}"}},"name":"{name}","fork":false,"private":false,"clone_url":"{}"}}"#,
			repo.path().display()
		)
	}

	fn owned_listing(owned: &str) -> Listing {
		Listing {
			owned: owned.to_owned(),
			contributed: BTreeMap::new(),
		}
	}

	fn listing(repos: &[(&str, &FixtureRepo)]) -> Listing {
		let entries: Vec<String> = repos
			.iter()
			.map(|(name, repo)| repo_json(OWNER, name, repo))
			.collect();
		owned_listing(&format!("[{}]", entries.join(",")))
	}

	fn config() -> Config {
		Config {
			owner: OWNER.to_owned(),
			authors: vec![ME.to_owned()],
			exclude_repos: Vec::new(),
			contributed: Vec::new(),
			top_n: 8,
			max_frames: 150,
			seconds_per_year: 5.0,
			languages: shipped_lists(),
		}
	}

	fn paths(dir: &TempDir) -> Paths {
		Paths {
			out: dir.path().join("out"),
			work: dir.path().join("work"),
			private: dir.path().join("private.json"),
		}
	}

	fn fixture(path: &str, content: &str) -> FixtureRepo {
		let repo = FixtureRepo::new();
		repo.commit(ME, WEEK_ONE_TS, &[(path, Some(content))]);
		repo.commit(ME, WEEK_TWO_TS, &[("extra.py", Some("x = 1\ny = 2\n"))]);
		repo
	}

	fn read(paths: &Paths, name: &str) -> Vec<u8> {
		std::fs::read(paths.out.join(name)).unwrap()
	}

	#[test]
	fn render_end_to_end_with_fake_fetch() {
		let alpha = fixture("a.rs", &"fn a() {}\n".repeat(40));
		let beta = fixture("b.c", &"int b;\n".repeat(30));
		let fetch = listing(&[("alpha", &alpha), ("beta", &beta)]);
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);

		render(&config(), &fetch, None, &paths).unwrap();
		let dark = read(&paths, "languages-dark.svg");
		let light = read(&paths, "languages-light.svg");
		assert!(dark.starts_with(b"<svg"));
		assert_ne!(dark, light);
		let cache_text = read(&paths, "cache/languages.json");
		let cache: Cache = serde_json::from_slice(&cache_text).unwrap();
		assert_eq!(cache.repos["me/alpha"].last_sha, alpha.head());
		assert_eq!(cache.repos["me/beta"].last_sha, beta.head());

		render(&config(), &fetch, None, &paths).unwrap();
		assert_eq!(read(&paths, "languages-dark.svg"), dark);
		assert_eq!(read(&paths, "languages-light.svg"), light);
		assert_eq!(read(&paths, "cache/languages.json"), cache_text);
	}

	#[test]
	fn render_merges_private_json() {
		let alpha = fixture("a.rs", &"fn a() {}\n".repeat(40));
		let fetch = listing(&[("alpha", &alpha)]);
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		render(&config(), &fetch, None, &paths).unwrap();
		let without = read(&paths, "languages-dark.svg");

		std::fs::write(
			&paths.private,
			r#"{"lines":{"2020-01-06":{"Haskell":5000}},"repos":{"2020-01-06":{"Haskell":1}}}"#,
		)
		.unwrap();
		render(&config(), &fetch, None, &paths).unwrap();
		let with = String::from_utf8(read(&paths, "languages-dark.svg")).unwrap();
		assert_ne!(with.as_bytes(), without);
		assert!(with.contains("Haskell"));
	}

	#[test]
	fn render_drops_repos_no_longer_listed() {
		let alpha = fixture("a.rs", &"fn a() {}\n".repeat(40));
		let beta = fixture("b.c", &"int b;\n".repeat(30));
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		render(
			&config(),
			&listing(&[("alpha", &alpha), ("beta", &beta)]),
			None,
			&paths,
		)
		.unwrap();

		render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).unwrap();
		assert_eq!(
			read_cache(&paths).repos.keys().collect::<Vec<_>>(),
			["me/alpha"]
		);
	}

	fn read_cache(paths: &Paths) -> Cache {
		serde_json::from_slice(&read(paths, "cache/languages.json")).unwrap()
	}

	fn language_totals(cache: &Cache) -> BTreeMap<&'static str, i64> {
		let mut totals = BTreeMap::new();
		for lines in cache::usage(cache).lines.values() {
			for (language, count) in lines {
				*totals.entry(*language).or_default() += count;
			}
		}
		totals
	}

	#[test]
	fn same_name_under_two_owners_counts_twice() {
		let mine = fixture("a.rs", &"fn a() {}\n".repeat(40));
		let theirs = fixture("b.c", &"int b;\n".repeat(30));
		let mut fetch = listing(&[("alpha", &mine)]);
		fetch.contributed.insert(
			"friend/alpha".to_owned(),
			repo_json("friend", "alpha", &theirs),
		);
		let mut config = config();
		config.contributed = vec!["friend/alpha".to_owned(), "friend/gone".to_owned()];
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);

		render(&config, &fetch, None, &paths).unwrap();

		let cache = read_cache(&paths);
		assert_eq!(
			cache.repos.keys().collect::<Vec<_>>(),
			["friend/alpha", "me/alpha"]
		);
		assert_eq!(cache.repos["me/alpha"].last_sha, mine.head());
		assert_eq!(cache.repos["friend/alpha"].last_sha, theirs.head());
		assert_eq!(
			language_totals(&cache),
			BTreeMap::from([("C", 30), ("Python", 4), ("Rust", 40)])
		);
		assert!(paths.work.join("me/alpha.git").exists());
		assert!(paths.work.join("friend/alpha.git").exists());

		config.contributed.clear();
		render(&config, &fetch, None, &paths).unwrap();
		assert_eq!(
			read_cache(&paths).repos.keys().collect::<Vec<_>>(),
			["me/alpha"]
		);
	}

	#[test]
	fn contributed_repo_already_owned_counts_once() {
		let alpha = fixture("a.rs", &"fn a() {}\n".repeat(40));
		let mut fetch = listing(&[("alpha", &alpha)]);
		fetch
			.contributed
			.insert("me/alpha".to_owned(), repo_json(OWNER, "alpha", &alpha));
		let mut config = config();
		config.contributed = vec!["me/alpha".to_owned()];
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);

		render(&config, &fetch, None, &paths).unwrap();

		let cache = read_cache(&paths);
		assert_eq!(cache.repos.keys().collect::<Vec<_>>(), ["me/alpha"]);
		assert_eq!(
			language_totals(&cache),
			BTreeMap::from([("Python", 2), ("Rust", 40)])
		);
		let info = |owner: &str, name: &str| RepoInfo {
			owner: owner.to_owned(),
			name: name.to_owned(),
			clone_url: String::new(),
		};
		let counted = counted_repos(
			vec![info(OWNER, "alpha"), info(OWNER, "beta")],
			vec![
				info(OWNER, "alpha"),
				info("friend", "alpha"),
				info("friend", "gamma"),
				info("friend", "alpha"),
			],
		);
		assert_eq!(
			counted.iter().map(RepoInfo::full_name).collect::<Vec<_>>(),
			["me/alpha", "me/beta", "friend/alpha", "friend/gamma"]
		);
	}

	#[test]
	fn render_reports_invalid_private_json() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::write(&paths.private, "not json").unwrap();
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
	}

	struct CountingFetch(std::cell::Cell<usize>);

	impl Fetch for CountingFetch {
		fn get(&self, _url: &str, _token: Option<&str>) -> Result<String> {
			self.0.set(self.0.get() + 1);
			Ok("[]".to_owned())
		}
	}

	#[test]
	fn render_validates_private_json_before_listing_repos() {
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::write(&paths.private, "not json").unwrap();
		let fetch = CountingFetch(std::cell::Cell::new(0));
		assert!(render(&config(), &fetch, None, &paths).is_err());
		assert_eq!(fetch.0.get(), 0);
		assert!(!paths.out.join("cache/languages.json").exists());
		std::fs::remove_file(&paths.private).unwrap();
		render(&config(), &fetch, None, &paths).unwrap();
		assert_eq!(fetch.0.get(), 1);
	}

	#[test]
	fn render_reports_unreadable_private_json() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::create_dir(&paths.private).unwrap();
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
	}

	#[test]
	fn render_reports_listing_and_sync_failures() {
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		assert!(render(&config(), &owned_listing("not json"), None, &paths).is_err());
		let broken = owned_listing(
			r#"[{"owner":{"login":"me"},"name":"gone","fork":false,"private":false,"clone_url":"/nonexistent/src"}]"#,
		);
		let error = render(&config(), &broken, None, &paths).unwrap_err();
		assert!(error.to_string().starts_with("git clone"), "{error}");
		let mut config = config();
		config.contributed = vec!["friend/broken".to_owned()];
		let unreachable = Listing {
			owned: "[]".to_owned(),
			contributed: BTreeMap::from([("friend/broken".to_owned(), "not json".to_owned())]),
		};
		assert!(render(&config, &unreachable, None, &paths).is_err());
	}

	#[test]
	fn render_rejects_language_unknown_to_linguist() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let mut config = config();
		config.languages.include.push("Nope".to_owned());
		let error = render(&config, &listing(&[("alpha", &alpha)]), None, &paths(&dir))
			.unwrap_err()
			.to_string();
		assert!(error.contains("Nope"), "{error}");
	}

	#[test]
	fn render_reports_unwritable_output() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let mut paths = paths(&dir);
		std::fs::write(dir.path().join("blocker"), "").unwrap();
		paths.out = dir.path().join("blocker");
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
	}

	#[test]
	fn render_reports_corrupt_cache_file() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::create_dir_all(paths.out.join("cache")).unwrap();
		std::fs::write(paths.out.join("cache/languages.json"), "not json").unwrap();
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
	}

	#[test]
	fn render_reports_unsavable_cache() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::create_dir_all(paths.out.join("cache/languages.json.tmp")).unwrap();
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
	}

	#[test]
	fn render_reports_unwritable_svg() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::create_dir_all(paths.out.join("languages-dark.svg")).unwrap();
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
	}

	#[test]
	fn render_reports_corrupt_clone() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		let fetch = listing(&[("alpha", &alpha)]);
		render(&config(), &fetch, None, &paths).unwrap();
		std::fs::remove_file(paths.out.join("cache/languages.json")).unwrap();
		let clone = paths.work.join("me/alpha.git");
		let blob = git::command(&clone)
			.args(["rev-parse", "HEAD:a.rs"])
			.output()
			.unwrap();
		let blob = String::from_utf8(blob.stdout).unwrap();
		let (fan_out, rest) = blob.trim().split_at(2);
		std::fs::remove_file(clone.join("objects").join(fan_out).join(rest)).unwrap();
		assert!(render(&config(), &fetch, None, &paths).is_err());
	}

	#[test]
	fn render_forwards_the_token() {
		struct Spy(std::cell::RefCell<Vec<Option<String>>>);
		impl Fetch for Spy {
			fn get(&self, _url: &str, token: Option<&str>) -> Result<String> {
				self.0.borrow_mut().push(token.map(str::to_owned));
				Ok("[]".to_owned())
			}
		}
		let dir = TempDir::new().unwrap();
		let spy = Spy(std::cell::RefCell::default());
		render(&config(), &spy, Some("tok"), &paths(&dir)).unwrap();
		assert_eq!(*spy.0.borrow(), [Some("tok".to_owned())]);
	}
}
