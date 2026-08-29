pub mod cli;
pub mod commands;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

pub fn run() -> Result<()> {
    run_with(Cli::parse())
}

pub fn run_with(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Hello(args) => commands::hello::run(args),
    }
}
