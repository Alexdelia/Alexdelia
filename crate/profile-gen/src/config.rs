use std::path::Path;

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
	pub owner: String,
	pub authors: Vec<String>,
	pub exclude_repos: Vec<String>,
	pub top_n: usize,
	pub max_frames: usize,
}

impl Config {
	pub fn parse(text: &str) -> Result<Self> {
		let config: Self = toml::from_str(text)?;
		if config.authors.is_empty() {
			return Err("config: authors must not be empty".into());
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
"#;

	#[test]
	fn parses_every_field() {
		let config = Config::parse(VALID).unwrap();
		assert_eq!(config.owner, "o");
		assert_eq!(config.authors, ["a@example.com"]);
		assert_eq!(config.exclude_repos, ["x"]);
		assert_eq!((config.top_n, config.max_frames), (3, 10));
	}

	#[test]
	fn config_rejects_empty_authors() {
		let text = VALID.replace(r#"["a@example.com"]"#, "[]");
		let error = Config::parse(&text).unwrap_err();
		assert!(error.to_string().contains("authors"), "{error}");
	}

	#[test]
	fn shipped_config_parses_once_authors_are_filled() {
		let text = include_str!("../config.toml")
			.replace("authors = []", r#"authors = ["a@example.com"]"#);
		assert_eq!(Config::parse(&text).unwrap().owner, "Alexdelia");
	}

	#[test]
	fn unknown_and_missing_fields_are_rejected() {
		assert!(Config::parse(&format!("{VALID}extra = 1")).is_err());
		assert!(Config::parse(r#"owner = "o""#).is_err());
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
