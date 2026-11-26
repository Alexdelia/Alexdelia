use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Result;

pub const MIN_SECONDS_PER_YEAR: f64 = 2.1;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
	pub owner: String,
	pub authors: Vec<String>,
	pub exclude_repos: Vec<String>,
	pub top_n: usize,
	pub max_frames: usize,
	pub seconds_per_year: f64,
	pub languages: LanguageLists,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageLists {
	pub include: Vec<String>,
	pub exclude: Vec<String>,
	pub ambiguous: BTreeMap<String, String>,
}

impl Config {
	pub fn parse(text: &str) -> Result<Self> {
		let config: Self = toml::from_str(text)?;
		if config.authors.is_empty() {
			return Err("config: authors must not be empty".into());
		}
		if config.top_n == 0 {
			return Err("config: top_n must be positive".into());
		}
		if config.max_frames == 0 {
			return Err("config: max_frames must be positive".into());
		}
		if !(config.seconds_per_year.is_finite() && config.seconds_per_year >= MIN_SECONDS_PER_YEAR)
		{
			return Err(format!(
				"config: seconds_per_year must be finite and at least {MIN_SECONDS_PER_YEAR}"
			)
			.into());
		}
		Ok(config)
	}

	pub fn load(path: &Path) -> Result<Self> {
		Self::parse(&std::fs::read_to_string(path)?)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	const VALID: &str = r#"
owner = "o"
authors = ["a@example.com"]
exclude_repos = ["x"]
top_n = 3
max_frames = 10
seconds_per_year = 2.5

[languages]
include = ["JSON"]
exclude = ["Perl"]
ambiguous = { ".h" = "C" }
"#;

	#[test]
	fn parses_every_field() {
		let config = Config::parse(VALID).unwrap();
		assert_eq!(config.owner, "o");
		assert_eq!(config.authors, ["a@example.com"]);
		assert_eq!(config.exclude_repos, ["x"]);
		assert_eq!((config.top_n, config.max_frames), (3, 10));
		assert!((config.seconds_per_year - 2.5).abs() < f64::EPSILON);
		assert_eq!(config.languages.include, ["JSON"]);
		assert_eq!(config.languages.exclude, ["Perl"]);
		assert_eq!(
			config.languages.ambiguous,
			BTreeMap::from([(".h".to_owned(), "C".to_owned())])
		);
	}

	fn rejection(from: &str, to: &str) -> String {
		Config::parse(&VALID.replace(from, to))
			.unwrap_err()
			.to_string()
	}

	#[test]
	fn config_rejects_zero_counts() {
		assert!(rejection("top_n = 3", "top_n = 0").contains("top_n"));
		assert!(rejection("max_frames = 10", "max_frames = 0").contains("max_frames"));
	}

	#[test]
	fn config_rejects_non_positive_or_non_finite_pace() {
		for bad in ["0", "-1", "-0.5", "inf", "nan", "2", "1.5"] {
			let error = rejection(
				"seconds_per_year = 2.5",
				&format!("seconds_per_year = {bad}"),
			);
			assert!(error.contains("seconds_per_year"), "{bad}: {error}");
		}
		let integer = Config::parse(&VALID.replace("2.5", "5")).unwrap();
		assert!((integer.seconds_per_year - 5.0).abs() < f64::EPSILON);
	}

	#[test]
	fn config_rejects_empty_authors() {
		let text = VALID.replace(r#"["a@example.com"]"#, "[]");
		let error = Config::parse(&text).unwrap_err();
		assert!(error.to_string().contains("authors"), "{error}");
	}

	#[test]
	fn shipped_config_parses_with_dense_pacing() {
		let config = Config::parse(include_str!("../config.toml")).unwrap();
		assert_eq!(config.owner, "Alexdelia");
		assert_eq!((config.top_n, config.max_frames), (16, 520));
		assert!((config.seconds_per_year - 5.0).abs() < f64::EPSILON);
		assert!(crate::language::Detector::new(&config.languages).is_ok());
	}

	#[test]
	fn unknown_and_missing_fields_are_rejected() {
		assert!(Config::parse(&format!("{VALID}extra = 1")).is_err());
		assert!(Config::parse(r#"owner = "o""#).is_err());
		assert!(Config::parse(&VALID.replace("exclude = [\"Perl\"]", "")).is_err());
		assert!(Config::parse(&format!("other = 1\n{VALID}")).is_err());
	}

	#[test]
	fn load_reads_file_and_reports_missing() {
		let dir = TempDir::new().unwrap();
		let path = dir.path().join("config.toml");
		assert!(Config::load(&path).is_err());
		std::fs::write(&path, VALID).unwrap();
		assert_eq!(Config::load(&path).unwrap().owner, "o");
	}
}
