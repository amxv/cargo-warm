use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print a greeting.
    Hello(HelloArgs),
}

#[derive(Debug, Args)]
pub struct HelloArgs {
    /// Name to greet.
    #[arg(default_value = "world")]
    pub name: String,
}
