pub mod cache;
pub mod cli;
pub mod commands;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

pub fn run() -> Result<()> {
    run_with_args(std::env::args().collect())
}

fn run_with_args(args: Vec<String>) -> Result<()> {
    let cli = Cli::parse_from(normalize_args(args));
    match cli.command {
        Command::Path(args) => commands::path::run(args),
        Command::Seed(args) => commands::seed::run(args),
        Command::Status => commands::status::run(),
        Command::Gc(args) => commands::gc::run(args),
    }
}

fn normalize_args(mut args: Vec<String>) -> Vec<String> {
    // Cargo invokes subcommands as `cargo-warm warm ...`. Direct invocation is
    // `cargo-warm ...`; support both without exposing two mental models.
    if args.get(1).is_some_and(|arg| arg == "warm") {
        args.remove(1);
    }
    args
}

#[cfg(test)]
mod tests {
    #[test]
    fn cargo_subcommand_prefix_is_removed() {
        let args = vec![
            "cargo-warm".to_string(),
            "warm".to_string(),
            "status".to_string(),
        ];
        let normalized = super::normalize_args(args);
        assert_eq!(normalized, ["cargo-warm", "status"]);
    }
}
