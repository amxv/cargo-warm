use std::{env, process::Command};

use anyhow::{Context, Result, bail};

use crate::{
    cli::CheckArgs,
    compiler::{RustcChannel, rustc_info},
    rustc_wrapper,
};

pub fn run(args: CheckArgs) -> Result<()> {
    let workspace = env::current_dir()?.canonicalize()?;
    let rustc = rustc_info(&workspace)?;
    if !rustc.relocatable_incremental_supported {
        bail!(
            "rustc {} predates relocatable incremental support; cargo warm check requires Rust 1.98 or newer",
            rustc.release
        );
    }

    let use_bootstrap = match rustc.channel {
        RustcChannel::Nightly | RustcChannel::Dev => false,
        RustcChannel::Stable | RustcChannel::Beta if args.unstable_bootstrap => true,
        RustcChannel::Stable | RustcChannel::Beta => bail!(
            "rustc {} exposes relocatable incremental state through an unstable compiler flag; use a nightly toolchain or explicitly pass --unstable-bootstrap",
            rustc.release
        ),
    };

    if env::var_os("RUSTC_WORKSPACE_WRAPPER").is_some() {
        bail!(
            "RUSTC_WORKSPACE_WRAPPER is already set; cargo-warm will not silently replace another workspace rustc wrapper"
        );
    }

    let wrapper = env::current_exe().context("failed to resolve the cargo-warm executable")?;
    if use_bootstrap {
        eprintln!(
            "cargo-warm: experimental stable-toolchain mode scopes RUSTC_BOOTSTRAP to each workspace crate and forbids unstable source features; Rust code can still observe that variable through env!/option_env!"
        );
    }
    eprintln!(
        "cargo-warm: relocatable incremental check with rustc {} ({:?})",
        rustc.release, rustc.channel
    );

    let mut command = Command::new("cargo");
    command
        .current_dir(&workspace)
        .arg("check")
        .args(args.cargo_args)
        .env("RUSTC_WORKSPACE_WRAPPER", &wrapper)
        .env(rustc_wrapper::marker_env(), "1");
    if use_bootstrap {
        command.env(rustc_wrapper::bootstrap_env(), "1");
    }

    let status = command.status().context("failed to start cargo check")?;
    if !status.success() {
        bail!("cargo check failed with {status}");
    }
    Ok(())
}
