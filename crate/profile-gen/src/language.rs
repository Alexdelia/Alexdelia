use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use serde::Deserialize;

use crate::config::LanguageLists;
use crate::error::Result;

pub const LINGUIST_DATA: &str = include_str!(env!("LINGUIST_LANGUAGES"));
const LINGUIST_PATH: &str = env!("LINGUIST_LANGUAGES");
const GITHUB_DEFAULT_COLOR: &str = "#cccccc";
const LOCKFILE_SUFFIX: &str = ".lock";
const LOCKFILE_NAMES: [&str; 3] = ["package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml"];

static LINGUIST: LazyLock<serde_json::Result<Linguist>> =
	LazyLock::new(|| Linguist::parse(LINGUIST_DATA));
static NO_LINGUIST: LazyLock<Linguist> = LazyLock::new(Linguist::default);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Language {
	pub name: &'static str,
	pub color: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
	Programming,
	Markup,
	Data,
	Prose,
}

#[derive(Debug, Deserialize)]
struct Entry {
	#[serde(rename = "type")]
	kind: Kind,
	color: Option<String>,
	group: Option<String>,
	#[serde(default)]
	extensions: Vec<String>,
	#[serde(default)]
	filenames: Vec<String>,
}

#[derive(Debug)]
struct Claim {
	name: String,
	primary: bool,
}

#[derive(Debug, Default)]
struct Linguist {
	entries: BTreeMap<String, Entry>,
	lowercase_names: HashMap<String, String>,
	filenames: HashMap<String, Vec<Claim>>,
	extensions: HashMap<String, Vec<Claim>>,
}

impl Linguist {
	fn parse(text: &str) -> serde_json::Result<Self> {
		serde_json::from_str(text).map(Self::index)
	}

	fn index(entries: BTreeMap<String, Entry>) -> Self {
		let mut linguist = Self::default();
		for (name, entry) in &entries {
			linguist
				.lowercase_names
				.insert(name.to_lowercase(), name.clone());
			for filename in &entry.filenames {
				linguist
					.filenames
					.entry(filename.clone())
					.or_default()
					.push(Claim {
						name: name.clone(),
						primary: false,
					});
			}
			for (position, extension) in entry.extensions.iter().enumerate() {
				linguist
					.extensions
					.entry(extension.to_lowercase())
					.or_default()
					.push(Claim {
						name: name.clone(),
						primary: position == 0,
					});
			}
		}
		linguist.entries = entries;
		linguist
	}

	fn canonical(&'static self, name: &str) -> Option<&'static str> {
		self.lowercase_names
			.get(&name.to_lowercase())
			.map(String::as_str)
	}

	fn root(&'static self, name: &'static str) -> &'static str {
		self.entries
			.get(name)
			.and_then(|entry| entry.group.as_deref())
			.unwrap_or(name)
	}

	fn language(&'static self, name: &'static str) -> Language {
		let root = self.root(name);
		Language {
			name: root,
			color: self
				.entries
				.get(root)
				.and_then(|entry| entry.color.as_deref())
				.unwrap_or(GITHUB_DEFAULT_COLOR),
		}
	}

	fn sole_root(
		&'static self,
		claims: impl Iterator<Item = &'static Claim>,
	) -> Option<&'static str> {
		let names: Vec<&'static str> = claims.map(|claim| claim.name.as_str()).collect();
		let first = *names.first()?;
		names
			.iter()
			.all(|name| self.root(name) == self.root(first))
			.then_some(first)
	}
}

fn linguist() -> &'static Linguist {
	LINGUIST.as_ref().unwrap_or(&NO_LINGUIST)
}

#[must_use]
pub fn canonical_name(name: &str) -> Option<&'static str> {
	linguist().canonical(name)
}

#[must_use]
pub fn language_by_name(name: &str) -> Option<Language> {
	canonical_name(name).map(|canonical| linguist().language(canonical))
}

#[derive(Debug)]
pub struct Detector {
	include: BTreeSet<&'static str>,
	exclude: BTreeSet<&'static str>,
	ambiguous: HashMap<String, &'static str>,
}

impl Detector {
	pub fn new(lists: &LanguageLists) -> Result<Self> {
		Self::with_data(&LINGUIST, lists)
	}

	fn with_data(
		parsed: &'static serde_json::Result<Linguist>,
		lists: &LanguageLists,
	) -> Result<Self> {
		checked(parsed).and_then(|linguist| Self::build(linguist, lists))
	}

	fn build(linguist: &'static Linguist, lists: &LanguageLists) -> Result<Self> {
		Ok(Self {
			include: known_names(linguist, "include", &lists.include)?,
			exclude: known_names(linguist, "exclude", &lists.exclude)?,
			ambiguous: lists
				.ambiguous
				.iter()
				.map(|(key, name)| claimed(linguist, key, name))
				.collect::<Result<_>>()?,
		})
	}

	#[must_use]
	pub fn detect(&self, path: &str) -> Option<Language> {
		let file_name = path.rsplit_once('/').map_or(path, |(_, name)| name);
		if is_lockfile(file_name) {
			return None;
		}
		self.identify(file_name).and_then(|name| self.count(name))
	}

	#[must_use]
	pub fn count(&self, name: &'static str) -> Option<Language> {
		let linguist = linguist();
		let entry = linguist.entries.get(name)?;
		let names = [name, linguist.root(name)];
		let excluded = names.iter().any(|name| self.exclude.contains(name));
		let included = names.iter().any(|name| self.include.contains(name));
		let counted = matches!(entry.kind, Kind::Programming | Kind::Markup) || included;
		(counted && !excluded).then(|| linguist.language(name))
	}

	fn identify(&self, file_name: &str) -> Option<&'static str> {
		let linguist = linguist();
		if let Some(claims) = linguist.filenames.get(file_name) {
			return self.resolve(file_name, claims);
		}
		let lowercase = file_name.to_lowercase();
		lowercase
			.match_indices('.')
			.map(|(at, _)| &lowercase[at..])
			.find_map(|extension| {
				linguist
					.extensions
					.get(extension)
					.map(|claims| (extension, claims))
			})
			.and_then(|(extension, claims)| self.resolve(extension, claims))
	}

	fn resolve(&self, key: &str, claims: &'static [Claim]) -> Option<&'static str> {
		let linguist = linguist();
		self.ambiguous
			.get(&key.to_lowercase())
			.copied()
			.or_else(|| linguist.sole_root(claims.iter()))
			.or_else(|| linguist.sole_root(claims.iter().filter(|claim| claim.primary)))
	}
}

fn checked(parsed: &'static serde_json::Result<Linguist>) -> Result<&'static Linguist> {
	parsed
		.as_ref()
		.map_err(|error| format!("Linguist languages data {LINGUIST_PATH}: {error}").into())
}

fn known_name(linguist: &'static Linguist, list: &str, name: &str) -> Result<&'static str> {
	linguist
		.canonical(name)
		.ok_or_else(|| format!("config: languages.{list} names {name}, unknown to Linguist").into())
}

fn known_names(
	linguist: &'static Linguist,
	list: &str,
	names: &[String],
) -> Result<BTreeSet<&'static str>> {
	names
		.iter()
		.map(|name| known_name(linguist, list, name))
		.collect()
}

fn claimed(linguist: &'static Linguist, key: &str, name: &str) -> Result<(String, &'static str)> {
	let target = known_name(linguist, "ambiguous", name)?;
	let lowercase = key.to_lowercase();
	let claims = linguist
		.extensions
		.get(&lowercase)
		.or_else(|| linguist.filenames.get(key));
	claims
		.into_iter()
		.flatten()
		.any(|claim| claim.name == target || linguist.root(&claim.name) == target)
		.then_some((lowercase, target))
		.ok_or_else(|| {
			format!(
				"config: languages.ambiguous {key} = {name}, but Linguist never gives {key} to {name}"
			)
			.into()
		})
}

fn is_lockfile(file_name: &str) -> bool {
	file_name.ends_with(LOCKFILE_SUFFIX) || LOCKFILE_NAMES.contains(&file_name)
}

#[cfg(test)]
pub(crate) fn every_language() -> impl Iterator<Item = Language> {
	linguist()
		.entries
		.keys()
		.map(|name| linguist().language(name))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn lists(include: &[&str], exclude: &[&str], ambiguous: &[(&str, &str)]) -> LanguageLists {
		LanguageLists {
			include: include.iter().map(|name| (*name).to_owned()).collect(),
			exclude: exclude.iter().map(|name| (*name).to_owned()).collect(),
			ambiguous: ambiguous
				.iter()
				.map(|(key, name)| ((*key).to_owned(), (*name).to_owned()))
				.collect(),
		}
	}

	fn detector(include: &[&str], exclude: &[&str], ambiguous: &[(&str, &str)]) -> Detector {
		Detector::new(&lists(include, exclude, ambiguous)).unwrap()
	}

	const fn named(name: &'static str, color: &'static str) -> Language {
		Language { name, color }
	}

	fn kind(name: &str) -> Kind {
		linguist().entries[name].kind
	}

	#[test]
	fn linguist_table_maps_names_colors_and_types() {
		assert_eq!(language_by_name("rust"), Some(named("Rust", "#dea584")));
		assert_eq!(language_by_name("C"), Some(named("C", "#555555")));
		assert_eq!(language_by_name("JSON"), Some(named("JSON", "#292929")));
		assert_eq!(
			language_by_name("markdown"),
			Some(named("Markdown", "#083fa1"))
		);
		assert_eq!(
			language_by_name("Roff Manpage"),
			Some(named("Roff", "#ecdebe"))
		);
		assert_eq!(
			language_by_name("TSX"),
			Some(named("TypeScript", "#3178c6"))
		);
		assert_eq!(language_by_name("nope"), None);
		assert_eq!(canonical_name("c++"), Some("C++"));
		assert_eq!(
			[kind("Rust"), kind("Roff"), kind("JSON"), kind("Markdown")],
			[Kind::Programming, Kind::Markup, Kind::Data, Kind::Prose]
		);

		let detector = crate::fixture::shipped_detector();
		let cases = [
			("src/main.rs", Some(named("Rust", "#dea584"))),
			("X.RS", Some(named("Rust", "#dea584"))),
			("a/b.c", Some(named("C", "#555555"))),
			("mlx/man/man1/mlx.1", Some(named("Roff", "#ecdebe"))),
			("mlx/man/man3/mlx_loop.3", Some(named("Roff", "#ecdebe"))),
			("Makefile", Some(named("Makefile", "#427819"))),
			("a/CMakeLists.txt", Some(named("CMake", "#DA3434"))),
			("config.cmake.in", Some(named("CMake", "#DA3434"))),
			("Dockerfile", Some(named("Dockerfile", "#384d54"))),
			("app.tsx", Some(named("TypeScript", "#3178c6"))),
			("README.md", Some(named("Markdown", "#083fa1"))),
			("data.json", Some(named("JSON", "#292929"))),
			("Cargo.toml", Some(named("TOML", "#9c4221"))),
		];
		for (path, expected) in cases {
			assert_eq!(detector.detect(path), expected, "{path}");
		}
	}

	#[test]
	fn data_and_prose_count_only_when_included() {
		let ambiguous = [(".md", "Markdown")];
		let without = detector(&[], &[], &ambiguous);
		let with = detector(&["markdown", "TOML", "JSON"], &[], &ambiguous);
		for path in ["README.md", "Cargo.toml", "tsconfig.json"] {
			assert_eq!(without.detect(path), None, "{path}");
		}
		assert_eq!(with.detect("README.md").map(|l| l.name), Some("Markdown"));
		assert_eq!(with.detect("Cargo.toml").map(|l| l.name), Some("TOML"));
		assert_eq!(with.detect("tsconfig.json").map(|l| l.name), Some("JSON"));
		assert_eq!(with.detect("a.ron"), None);
		assert_eq!(without.detect("a.c").map(|l| l.name), Some("C"));
	}

	#[test]
	fn exclude_removes_any_language() {
		let detector = detector(&["TOML"], &["C", "toml", "TypeScript"], &[]);
		for path in ["a.c", "Cargo.toml", "app.ts", "app.tsx"] {
			assert_eq!(detector.detect(path), None, "{path}");
		}
		assert_eq!(detector.count("C"), None);
		assert_eq!(detector.detect("a.py").map(|l| l.name), Some("Python"));
	}

	#[test]
	fn unknown_extensions_never_count() {
		let detector = detector(&["Text", "JSON"], &[], &[]);
		for path in ["a.rc", "x.yuck", "a.in", "noext", "dir/", "a.", ""] {
			assert_eq!(detector.detect(path), None, "{path}");
		}
		assert_eq!(detector.count("Nope"), None);
	}

	#[test]
	fn lockfiles_never_count() {
		let detector = detector(&["TOML", "JSON", "YAML"], &[], &[]);
		for path in [
			"Cargo.lock",
			"flake.lock",
			"a/package-lock.json",
			"npm-shrinkwrap.json",
			"pnpm-lock.yaml",
		] {
			assert_eq!(detector.detect(path), None, "{path}");
		}
	}

	#[test]
	fn ambiguous_extension_follows_config_then_primary() {
		let plain = detector(&[], &[], &[]);
		assert_eq!(plain.detect("inc/x.h"), None);
		assert_eq!(plain.detect("a.pl"), None);
		assert_eq!(plain.detect("a.txt"), None);
		assert_eq!(plain.detect("app.ts").map(|l| l.name), Some("TypeScript"));
		assert_eq!(plain.detect("index.php").map(|l| l.name), Some("PHP"));
		assert_eq!(plain.detect("boot.s").map(|l| l.name), Some("Assembly"));

		let configured = detector(&[], &[], &[(".h", "C"), (".PL", "perl")]);
		assert_eq!(configured.detect("inc/x.h").map(|l| l.name), Some("C"));
		assert_eq!(configured.detect("X.H").map(|l| l.name), Some("C"));
		assert_eq!(configured.detect("a.pl").map(|l| l.name), Some("Perl"));
		assert_eq!(configured.detect("x.h.in").map(|l| l.name), Some("C"));
	}

	#[test]
	fn config_lists_must_name_linguist_languages() {
		for (bad, list) in [
			(lists(&["Nope"], &[], &[]), "include"),
			(lists(&[], &["Nope"], &[]), "exclude"),
			(lists(&[], &[], &[(".h", "Nope")]), "ambiguous"),
		] {
			let error = Detector::new(&bad).unwrap_err().to_string();
			assert!(error.contains(list) && error.contains("Nope"), "{error}");
		}
	}

	static BROKEN: LazyLock<serde_json::Result<Linguist>> =
		LazyLock::new(|| Linguist::parse("not json"));

	#[test]
	fn broken_linguist_data_is_an_error_naming_it() {
		let error = Detector::with_data(&BROKEN, &lists(&[], &[], &[]))
			.unwrap_err()
			.to_string();
		assert!(error.contains("Linguist"), "{error}");
		assert!(error.contains(env!("LINGUIST_LANGUAGES")), "{error}");
		assert!(Detector::with_data(&LINGUIST, &lists(&[], &[], &[])).is_ok());
	}

	#[test]
	fn ambiguous_target_must_claim_the_extension() {
		for (key, name) in [(".h", "Rust"), (".nope", "C"), ("Makefile", "Rust")] {
			let error = Detector::new(&lists(&[], &[], &[(key, name)]))
				.unwrap_err()
				.to_string();
			assert!(error.contains(key) && error.contains(name), "{error}");
		}
		for (key, name) in [(".H", "c++"), (".s", "Assembly"), (".1", "Roff Manpage")] {
			assert!(
				Detector::new(&lists(&[], &[], &[(key, name)])).is_ok(),
				"{key}"
			);
		}
		let makefile = detector(&[], &[], &[("Makefile", "Makefile")]);
		assert_eq!(
			makefile.detect("Makefile").map(|l| l.name),
			Some("Makefile")
		);
	}

	#[test]
	fn shipped_linguist_data_parses_with_known_groups() {
		assert!(Linguist::parse(LINGUIST_DATA).is_ok());
		assert!(Linguist::parse("not json").is_err());
		for entry in linguist().entries.values() {
			if let Some(group) = &entry.group {
				assert!(linguist().entries.contains_key(group), "{group}");
			}
		}
		assert!(every_language().count() > 500);
	}

	#[test]
	fn language_without_linguist_color_gets_github_default() {
		assert_eq!(
			language_by_name("Linker Script"),
			Some(named("Linker Script", "#cccccc"))
		);
	}
}
