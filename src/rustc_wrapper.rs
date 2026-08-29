use std::{
    env,
    ffi::OsString,
    process::{Command, ExitCode},
};

const WRAPPER_MARKER: &str = "CARGO_WARM_RUSTC_WRAPPER";
const USE_BOOTSTRAP: &str = "CARGO_WARM_USE_BOOTSTRAP";
const VIRTUAL_CWD: &str = "/cargo-warm/v1/workspace";

pub fn is_wrapper_invocation() -> bool {
    env::var_os(WRAPPER_MARKER).is_some()
}

pub fn run() -> ExitCode {
    match run_inner(env::args_os().collect()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("cargo-warm rustc wrapper error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_inner(args: Vec<OsString>) -> Result<ExitCode, String> {
    let mut args = args.into_iter();
    let _wrapper = args.next();
    let rustc = args
        .next()
        .ok_or_else(|| "Cargo did not pass a rustc executable".to_owned())?;
    let rustc_args: Vec<OsString> = args.collect();

    // Cargo can route rustc capability/version probes (for example `rustc
    // -vV`) through RUSTC_WORKSPACE_WRAPPER as well. Those are not crate
    // compilations and must remain transparent. Cargo crate compilations
    // always carry `--crate-name`, which is also the identity we need for the
    // scoped stable-toolchain bootstrap below.
    let workspace_compile = crate_name(&rustc_args).is_some();
    let mut command = Command::new(&rustc);
    command.args(&rustc_args);
    command
        .env_remove(WRAPPER_MARKER)
        .env_remove(USE_BOOTSTRAP)
        .env_remove("RUSTC_WORKSPACE_WRAPPER");

    if workspace_compile {
        if env::var_os(USE_BOOTSTRAP).is_some() {
            let crate_name = crate_name(&rustc_args)
                .expect("workspace_compile is true only when --crate-name is present");
            // Scope bootstrap to this one workspace crate instead of turning
            // the whole build into a nightly-like compilation. Then forbid
            // unstable source features so the only unstable capability we
            // intentionally consume is rustc's relocation flag itself.
            command
                .env("RUSTC_BOOTSTRAP", crate_name)
                .arg("-Zallow-features=")
                .args(["-F", "unstable-features"]);
        }
        command.arg(format!("-Zremap-cwd-prefix={VIRTUAL_CWD}"));
    }

    let status = command
        .status()
        .map_err(|error| format!("failed to execute {:?}: {error}", rustc))?;
    Ok(match status.code() {
        Some(code) if (0..=255).contains(&code) => ExitCode::from(code as u8),
        Some(_) | None => ExitCode::FAILURE,
    })
}

fn crate_name(args: &[OsString]) -> Option<&std::ffi::OsStr> {
    args.windows(2)
        .find_map(|pair| (pair[0] == "--crate-name").then_some(pair[1].as_os_str()))
}

pub(crate) fn marker_env() -> &'static str {
    WRAPPER_MARKER
}

pub(crate) fn bootstrap_env() -> &'static str {
    USE_BOOTSTRAP
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    #[test]
    fn finds_workspace_crate_name() {
        let args = [
            OsString::from("--edition=2024"),
            OsString::from("--crate-name"),
            OsString::from("my_workspace_crate"),
            OsString::from("src/lib.rs"),
        ];
        assert_eq!(
            super::crate_name(&args),
            Some(std::ffi::OsStr::new("my_workspace_crate"))
        );
    }

    #[test]
    fn rustc_version_probe_is_not_a_compile_invocation() {
        let args = [OsString::from("-vV")];
        assert_eq!(super::crate_name(&args), None);
    }
}
