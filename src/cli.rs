use std::{ffi::OsString, path::PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::config::PrimeMode;

#[derive(Debug, Parser)]
#[command(
    name = "cargo warm",
    version,
    about = "Fork warm Cargo build state into private worktree caches"
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
    /// Diagnose the project/worktree and recommend a cargo-warm profile.
    Doctor(DoctorArgs),
    /// Run cargo check with relocatable workspace incremental state.
    Check(CheckArgs),
    /// Show cache roots created by cargo-warm.
    Status,
    /// Remove cargo-warm cache roots whose workspaces no longer exist.
    Gc(GcArgs),
}

#[derive(Debug, Args)]
pub struct PathArgs {
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Explicit project config path. Defaults to `.agents/.cargo-warm.toml` at the Git root when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Cargo workspace manifest to inspect. Repeat to override configured manifests.
    #[arg(long = "manifest-path")]
    pub manifests: Vec<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SeedArgs {
    /// Named built-in or project-local profile. Defaults to the project's configured profile.
    #[arg(long)]
    pub profile: Option<String>,
    /// Explicit project config path. Defaults to `.agents/.cargo-warm.toml` at the Git root when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Warm checkout to seed from. Defaults to the nearest compatible worktree with usable cache state.
    #[arg(long = "from")]
    pub source: Option<PathBuf>,
    /// Worktree that should receive the private cache.
    #[arg(long = "to", default_value = ".")]
    pub destination: PathBuf,
    /// Cargo workspace manifest to seed. Repeat for repositories with several Cargo workspaces.
    #[arg(long = "manifest-path")]
    pub manifests: Vec<PathBuf>,
    /// Also seed Cargo's target directory when Cargo reports a separate build_directory.
    #[arg(long)]
    pub include_target: bool,
    /// Allow a normal physical copy when filesystem COW/reflink is unavailable.
    #[arg(long)]
    pub copy_fallback: bool,
    /// COW-clone an additional workspace-relative cache path (repeatable).
    #[arg(long = "seed-path")]
    pub seed_paths: Vec<PathBuf>,
    /// Disable safe freshness synchronization and keep checkout mtimes unchanged.
    #[arg(long)]
    pub no_freshness_rebase: bool,
    /// After seeding, force one no-content-change relocatable rustc session so
    /// the first real edit starts from destination-native incremental state.
    /// This is a compatibility shortcut for `--prime-mode package`.
    #[arg(long, conflicts_with = "prime_mode")]
    pub prime: bool,
    /// Override the profile's priming strategy.
    #[arg(long, value_enum)]
    pub prime_mode: Option<PrimeMode>,
    /// Allow relocatable priming on stable/beta Rust for this invocation.
    #[arg(long)]
    pub unstable_bootstrap: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Profile to evaluate. Defaults to the project's configured profile.
    #[arg(long)]
    pub profile: Option<String>,
    /// Explicit project config path. Defaults to `.agents/.cargo-warm.toml` at the Git root when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Warm checkout to compare against. Defaults to the nearest compatible worktree.
    #[arg(long = "from")]
    pub source: Option<PathBuf>,
    /// Worktree to diagnose.
    #[arg(long = "to", default_value = ".")]
    pub destination: PathBuf,
    /// Cargo workspace manifests to inspect. Repeat for repositories with multiple workspaces.
    #[arg(long = "manifest-path")]
    pub manifests: Vec<PathBuf>,
    /// Run `cargo check` with Cargo fingerprint logging and report actual rebuild reasons.
    #[arg(long)]
    pub probe: bool,
    /// Emit a machine-readable report.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Named built-in or project-local profile. Defaults to the project's configured profile.
    #[arg(long)]
    pub profile: Option<String>,
    /// Explicit project config path. Defaults to `.agents/.cargo-warm.toml` at the Git root when present.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Allow the unstable rustc relocation flag on a stable toolchain by setting
    /// RUSTC_BOOTSTRAP=1 for workspace rustc invocations. Rust code can observe
    /// that environment variable through env!/option_env!, so this is never implicit.
    #[arg(long)]
    pub unstable_bootstrap: bool,
    /// Arguments forwarded to `cargo check`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub cargo_args: Vec<OsString>,
}

#[derive(Debug, Args)]
pub struct GcArgs {
    #[arg(long)]
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn check_forwards_cargo_flags_without_separator() {
        let cli = Cli::parse_from([
            "cargo-warm",
            "check",
            "--unstable-bootstrap",
            "--manifest-path",
            "crates/app/Cargo.toml",
            "--workspace",
        ]);
        let Command::Check(args) = cli.command else {
            panic!("expected check command");
        };
        assert!(args.unstable_bootstrap);
        assert_eq!(args.profile, None);
        assert_eq!(
            args.cargo_args,
            [
                OsString::from("--manifest-path"),
                OsString::from("crates/app/Cargo.toml"),
                OsString::from("--workspace"),
            ]
        );
    }

    #[test]
    fn seed_prime_accepts_explicit_stable_bootstrap() {
        let cli = Cli::parse_from([
            "cargo-warm",
            "seed",
            "--prime",
            "--unstable-bootstrap",
            "--manifest-path",
            "crates/app/Cargo.toml",
        ]);
        let Command::Seed(args) = cli.command else {
            panic!("expected seed command");
        };
        assert!(args.prime);
        assert_eq!(args.prime_mode, None);
        assert!(args.unstable_bootstrap);
        assert!(
            args.manifests
                .iter()
                .any(|path| path.ends_with("crates/app/Cargo.toml"))
        );
    }

    #[test]
    fn seed_accepts_named_project_profile() {
        let cli = Cli::parse_from([
            "cargo-warm",
            "seed",
            "--profile",
            "agent",
            "--config",
            "config/cargo-warm.toml",
        ]);
        let Command::Seed(args) = cli.command else {
            panic!("expected seed command");
        };
        assert_eq!(args.profile.as_deref(), Some("agent"));
        assert_eq!(
            args.config.as_deref(),
            Some(std::path::Path::new("config/cargo-warm.toml"))
        );
    }
}
