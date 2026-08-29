use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "cargo warm",
    version,
    about = "Fork warm Cargo build state into isolated worktrees"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show Cargo's resolved cache paths for a workspace.
    Path(PathArgs),
    /// Fork warm build state from one checkout into another.
    Seed(SeedArgs),
    /// Show cache roots created by cargo-warm.
    Status,
    /// Remove cargo-warm cache roots whose workspaces no longer exist.
    Gc(GcArgs),
}

#[derive(Debug, Args)]
pub struct PathArgs {
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    #[arg(long = "manifest-path", default_value = "Cargo.toml")]
    pub manifests: Vec<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SeedArgs {
    #[arg(long = "from")]
    pub source: Option<PathBuf>,
    #[arg(long = "to", default_value = ".")]
    pub destination: PathBuf,
    #[arg(long = "manifest-path", default_value = "Cargo.toml")]
    pub manifests: Vec<PathBuf>,
    /// Also seed Cargo's target directory when Cargo reports a separate build_directory.
    /// On modern Cargo, build_directory contains the expensive reusable compiler state,
    /// while target_directory is mostly final/link output and is skipped by default.
    #[arg(long)]
    pub include_target: bool,
    /// Allow a normal physical copy when filesystem COW/reflink is unavailable.
    #[arg(long)]
    pub copy_fallback: bool,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    #[arg(long)]
    pub dry_run: bool,
}
