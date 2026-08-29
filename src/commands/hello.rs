use anyhow::Result;

use crate::cli::HelloArgs;

pub fn run(args: HelloArgs) -> Result<()> {
    println!("Hello, {}!", args.name.trim());
    Ok(())
}
