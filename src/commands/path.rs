use anyhow::Result;

use crate::{cache, cli::PathArgs};

pub fn run(args: PathArgs) -> Result<()> {
    let workspace = cache::canonical_dir(&args.workspace)?;
    let paths = cache::resolve_manifests(&workspace, &args.manifests)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&paths)?);
        return Ok(());
    }
    for item in paths {
        println!("manifest: {}", item.manifest.display());
        println!("  workspace: {}", item.workspace.display());
        match item.build_directory {
            Some(path) => println!("  build:  {}", path.display()),
            None => println!("  build:  <not reported by this Cargo>"),
        }
        println!("  target: {}", item.target_directory.display());
    }
    Ok(())
}
