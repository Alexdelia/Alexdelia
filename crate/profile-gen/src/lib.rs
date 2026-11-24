pub mod attributes;
pub mod cache;
pub mod error;
#[cfg(test)]
mod fixture;
mod git;
pub mod github;
pub mod history;
pub mod icons;
pub mod language;
pub mod render;
pub mod theme;
pub mod timeline;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "profile-gen", about = "Generate the profile SVGs")]
pub struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
	Render,
	Private,
}

pub fn run(cli: &Cli) -> Result<(), String> {
	let name = match cli.command {
		Command::Render => "render",
		Command::Private => "private",
	};
	Err(format!("{name}: not implemented"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::CommandFactory;

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
	fn subcommands_are_not_implemented() {
		for name in ["render", "private"] {
			let cli = Cli::try_parse_from(["profile-gen", name]).unwrap();
			assert_eq!(run(&cli), Err(format!("{name}: not implemented")));
		}
	}
}
