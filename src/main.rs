use std::process::ExitCode;

fn main() -> ExitCode {
    if cargo_warm::rustc_wrapper::is_wrapper_invocation() {
        return cargo_warm::rustc_wrapper::run();
    }

    match cargo_warm::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
