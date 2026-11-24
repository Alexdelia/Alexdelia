use clap::Parser;
use profile_gen::error::Result;
use profile_gen::github::Ureq;
use profile_gen::{Cli, execute};

fn main() -> Result<()> {
	let token = std::env::var("GITHUB_TOKEN").ok();
	execute(&Cli::parse(), &Ureq, token.as_deref())
}
