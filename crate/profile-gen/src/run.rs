use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::cache::{self, Usage};
use crate::config::Config;
use crate::error::Result;
use crate::github::{Fetch, list_public, sync_clone};
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

fn refresh_cache(
	config: &Config,
	fetch: &impl Fetch,
	token: Option<&str>,
	work: &Path,
	cache_path: &Path,
) -> Result<cache::Cache> {
	let rules_hash = cache::rules_hash(&config.authors, &config.exclude_repos);
	let mut cache = cache::load(cache_path, &rules_hash)?;
	let repos = list_public(fetch, &config.owner, token, &config.exclude_repos)?;
	for repo in &repos {
		let clone = sync_clone(repo, work)?;
		cache::update(&mut cache, &repo.name, &clone, &config.authors)?;
	}
	let names: BTreeSet<String> = repos.into_iter().map(|repo| repo.name).collect();
	cache::retain(&mut cache, &names);
	Ok(cache)
}

pub fn render(
	config: &Config,
	fetch: &impl Fetch,
	token: Option<&str>,
	paths: &Paths,
) -> Result<()> {
	let cache_dir = paths.out.join(CACHE_DIR);
	std::fs::create_dir_all(&cache_dir)?;
	let cache_path = cache_dir.join(CACHE_FILE);
	let cache = refresh_cache(config, fetch, token, &paths.work, &cache_path)?;
	cache::save(&cache, &cache_path)?;

	let usage = cache::usage(&cache).merge(read_private(&paths.private)?);
	let timeline = timeline(&usage, config.top_n, config.max_frames);
	for (variant, file) in [
		(Variant::Dark, "languages-dark.svg"),
		(Variant::Light, "languages-light.svg"),
	] {
		std::fs::write(paths.out.join(file), render_languages(&timeline, variant))?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cache::Cache;
	use crate::fixture::FixtureRepo;
	use crate::git;
	use tempfile::TempDir;

	const ME: &str = "me@example.com";
	const WEEK_ONE_TS: i64 = 1_577_836_800;
	const WEEK_TWO_TS: i64 = 1_578_441_600;

	struct Listing(String);

	impl Fetch for Listing {
		fn get(&self, url: &str, _token: Option<&str>) -> Result<String> {
			Ok(if url.ends_with("page=1") {
				self.0.clone()
			} else {
				"[]".to_owned()
			})
		}
	}

	fn listing(repos: &[(&str, &FixtureRepo)]) -> Listing {
		let entries: Vec<String> = repos
			.iter()
			.map(|(name, repo)| {
				format!(
					r#"{{"name":"{name}","fork":false,"private":false,"clone_url":"{}"}}"#,
					repo.path().display()
				)
			})
			.collect();
		Listing(format!("[{}]", entries.join(",")))
	}

	fn config() -> Config {
		Config {
			owner: "o".to_owned(),
			authors: vec![ME.to_owned()],
			exclude_repos: Vec::new(),
			top_n: 8,
			max_frames: 150,
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
		assert_eq!(cache.repos["alpha"].last_sha, alpha.head());
		assert_eq!(cache.repos["beta"].last_sha, beta.head());

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
			r#"{"weeks":{"2020-01-06":{"Haskell":5000}},"first_use":{"Haskell":["2020-01-06"]}}"#,
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
		let cache: Cache = serde_json::from_slice(&read(&paths, "cache/languages.json")).unwrap();
		assert_eq!(cache.repos.keys().collect::<Vec<_>>(), ["alpha"]);
	}

	#[test]
	fn render_reports_invalid_private_json() {
		let alpha = fixture("a.rs", "fn a() {}\n");
		let dir = TempDir::new().unwrap();
		let paths = paths(&dir);
		std::fs::write(&paths.private, "not json").unwrap();
		assert!(render(&config(), &listing(&[("alpha", &alpha)]), None, &paths).is_err());
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
		assert!(render(&config(), &Listing("not json".to_owned()), None, &paths).is_err());
		let broken = Listing(
			r#"[{"name":"gone","fork":false,"private":false,"clone_url":"/nonexistent/src"}]"#
				.to_owned(),
		);
		assert!(render(&config(), &broken, None, &paths).is_err());
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
		let clone = paths.work.join("alpha.git");
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
