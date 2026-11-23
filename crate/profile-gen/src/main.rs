use clap::Parser;
use profile_gen::{Cli, run};

fn main() -> Result<(), String> {
	run(&Cli::parse())
}
