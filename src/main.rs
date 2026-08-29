use std::process::ExitCode;

fn main() -> ExitCode {
    match cargo_warm::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
