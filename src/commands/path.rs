use anyhow::Result;

use crate::{
    cache,
    cli::PathArgs,
    config::{self, SeedOverrides},
};

pub fn run(args: PathArgs) -> Result<()> {
    let workspace = cache::canonical_dir(&args.workspace)?;
    let effective = config::resolve_seed(
        &workspace,
        SeedOverrides {
            config: args.config,
            manifests: args.manifests,
            ..SeedOverrides::default()
        },
    )?;
    let paths = cache::resolve_manifests(&workspace, &effective.manifests)?;
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
