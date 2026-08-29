use std::{ffi::OsString, path::PathBuf};

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
    /// Explain whether a seeded worktree should stay warm and why Cargo rebuilds.
    Doctor(DoctorArgs),
    /// Run cargo check with experimental relocatable workspace incremental state.
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
    #[arg(long)]
    pub include_target: bool,
    /// Allow a normal physical copy when filesystem COW/reflink is unavailable.
    #[arg(long)]
    pub copy_fallback: bool,
    /// COW-clone an additional workspace-relative cache path (repeatable).
    #[arg(long = "seed-path")]
    pub seed_paths: Vec<PathBuf>,
    /// Disable 3B freshness rebasing and keep checkout mtimes unchanged.
    #[arg(long)]
    pub no_freshness_rebase: bool,
    /// After seeding, force one no-content-change relocatable rustc session so
    /// the first real edit starts from destination-native incremental state.
    #[arg(long)]
    pub prime: bool,
    /// Allow --prime to use rustc's unstable relocation flag on stable/beta.
    #[arg(long, requires = "prime")]
    pub unstable_bootstrap: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Warm checkout to compare against. Defaults to the nearest compatible worktree.
    #[arg(long = "from")]
    pub source: Option<PathBuf>,
    /// Worktree to diagnose.
    #[arg(long = "to", default_value = ".")]
    pub destination: PathBuf,
    /// Cargo workspace manifests to inspect. Repeat for repositories with multiple workspaces.
    #[arg(long = "manifest-path", default_value = "Cargo.toml")]
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
        assert!(args.unstable_bootstrap);
        assert!(
            args.manifests
                .iter()
                .any(|path| path.ends_with("crates/app/Cargo.toml"))
        );
    }
}
