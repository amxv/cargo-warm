use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail};

use crate::{
    cache::{self, CacheKind},
    cli::SeedArgs,
};

pub fn run(args: SeedArgs) -> Result<()> {
    let destination = cache::canonical_dir(&args.destination)?;
    let source = match args.source {
        Some(source) => cache::canonical_dir(&source)?,
        None => cache::find_main_worktree(&destination)?.ok_or_else(|| {
            anyhow!("could not find a separate Git worktree on branch main; pass --from explicitly")
        })?,
    };
    if source == destination {
        bail!("source and destination workspaces are identical");
    }

    cache::assert_workspace_quiescent(&source)?;
    cache::assert_workspace_quiescent(&destination)?;
    cache::assert_compatible_toolchains(&source, &destination)?;
    let source_paths = cache::resolve_manifests(&source, &args.manifests)?;
    let destination_paths = cache::resolve_manifests(&destination, &args.manifests)?;
    if source_paths.len() != destination_paths.len() {
        bail!("source and destination resolved different manifest counts");
    }

    let strategy = cache::clone_strategy(args.copy_fallback)?;
    let mut registry = cache::read_registry()?;
    let mut seen = BTreeSet::new();
    let mut created = 0usize;

    for (source_paths, destination_paths) in source_paths.iter().zip(&destination_paths) {
        if source_paths.manifest.file_name() != destination_paths.manifest.file_name() {
            bail!("source and destination manifest layouts differ");
        }

        if let (Some(from), Some(to)) = (
            source_paths.build_directory.as_ref(),
            destination_paths.build_directory.as_ref(),
        ) {
            let unseen = seen.insert(to.clone());
            if unseen
                && cache::seed_one(
                    CacheKind::Build,
                    from,
                    to,
                    &source,
                    &destination,
                    &destination_paths.manifest,
                    strategy,
                    &mut registry,
                )?
            {
                created += 1;
            }
        }

        // Cargo versions without build_directory keep their build cache under
        // target_directory, so target remains the compatibility fallback.
        // When build_directory exists separately, avoid cloning final/link
        // outputs unless the caller explicitly asks for them.
        if args.include_target || source_paths.build_directory.is_none() {
            let from = &source_paths.target_directory;
            let to = &destination_paths.target_directory;
            if seen.insert(to.clone())
                && cache::seed_one(
                    CacheKind::Target,
                    from,
                    to,
                    &source,
                    &destination,
                    &destination_paths.manifest,
                    strategy,
                    &mut registry,
                )?
            {
                created += 1;
            }
        }
    }

    cache::write_registry(&registry)?;
    if created == 0 {
        println!("cargo-warm: nothing seeded");
    } else {
        println!("cargo-warm: seeded {created} private cache root(s)");
    }
    Ok(())
}
