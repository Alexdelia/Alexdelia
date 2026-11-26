pub mod attributes;
pub mod cache;
pub mod config;
pub mod error;
#[cfg(test)]
mod fixture;
mod git;
pub mod github;
pub mod history;
pub mod icons;
pub mod language;
pub mod render;
pub mod run;
pub mod theme;
pub mod timeline;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::error::Result;
use crate::github::Fetch;
use crate::run::{Paths, render};

const PRIVATE_FILE: &str = "private.json";

#[derive(Debug, Parser)]
#[command(name = "profile-gen", about = "Generate the profile SVGs")]
pub struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
	Render {
		#[arg(long)]
		config: PathBuf,
		#[arg(long)]
		out: PathBuf,
		#[arg(long)]
		work: PathBuf,
	},
	Private,
}

pub fn execute(cli: &Cli, fetch: &impl Fetch, token: Option<&str>) -> Result<()> {
	match &cli.command {
		Command::Render { config, out, work } => {
			let paths = Paths {
				out: out.clone(),
				work: work.clone(),
				private: config.with_file_name(PRIVATE_FILE),
			};
			render(&Config::load(config)?, fetch, token, &paths)
		}
		Command::Private => Err("private: not implemented".into()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::CommandFactory;
	use tempfile::TempDir;

	struct NoNetwork;

	impl Fetch for NoNetwork {
		fn get(&self, _url: &str, _token: Option<&str>) -> Result<String> {
			Ok("[]".to_owned())
		}
	}

	#[test]
	fn help_lists_subcommands() {
		let command = Cli::command();
		let name_list: Vec<_> = command
			.get_subcommands()
			.map(clap::Command::get_name)
			.collect();
		assert_eq!(name_list, ["render", "private"]);
	}

	#[test]
	fn private_is_not_implemented() {
		let cli = Cli::try_parse_from(["profile-gen", "private"]).unwrap();
		let error = execute(&cli, &NoNetwork, None).unwrap_err();
		assert_eq!(error.to_string(), "private: not implemented");
	}

	#[test]
	fn render_reports_missing_config() {
		let dir = TempDir::new().unwrap();
		let missing = dir.path().join("missing.toml");
		let cli = Cli::try_parse_from([
			"profile-gen".as_ref(),
			"render".as_ref(),
			"--config".as_ref(),
			missing.as_os_str(),
			"--out".as_ref(),
			dir.path().as_os_str(),
			"--work".as_ref(),
			dir.path().as_os_str(),
		])
		.unwrap();
		assert!(execute(&cli, &NoNetwork, None).is_err());
	}

	#[test]
	fn render_requires_its_arguments() {
		assert!(Cli::try_parse_from(["profile-gen", "render"]).is_err());
	}

	#[test]
	fn render_reads_config_and_sibling_private_json() {
		let dir = TempDir::new().unwrap();
		let config = dir.path().join("config.toml");
		std::fs::write(
			&config,
			"owner = \"o\"\nauthors = [\"a@example.com\"]\nexclude_repos = []\ntop_n = 8\nmax_frames = 150\nseconds_per_year = 5\n[languages]\ninclude = []\nexclude = []\nambiguous = {}\n",
		)
		.unwrap();
		std::fs::write(dir.path().join(PRIVATE_FILE), "not json").unwrap();
		let out = dir.path().join("out");
		let work = dir.path().join("work");
		let args = |extra: &[&std::path::Path]| {
			let mut list = vec!["profile-gen".into(), "render".into()];
			for (flag, path) in ["--config", "--out", "--work"].iter().zip(extra) {
				list.push((*flag).into());
				list.push(path.as_os_str().to_owned());
			}
			Cli::try_parse_from(list).unwrap()
		};
		let cli = args(&[&config, &out, &work]);

		assert!(execute(&cli, &NoNetwork, None).is_err());
		std::fs::remove_file(dir.path().join(PRIVATE_FILE)).unwrap();
		execute(&cli, &NoNetwork, None).unwrap();
		assert!(out.join("languages-dark.svg").exists());
	}
}
