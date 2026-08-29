use std::collections::BTreeSet;

use anyhow::{Context, Result, anyhow, bail};

use crate::{
    cache::{self, CacheKind},
    cli::SeedArgs,
    freshness, git, prime,
};

pub fn run(args: SeedArgs) -> Result<()> {
    let destination = cache::canonical_dir(&args.destination)?;
    cache::assert_workspace_quiescent(&destination)?;
    let (source, source_reason) = match args.source {
        Some(source) => (
            cache::canonical_dir(&source)?,
            "explicit --from".to_string(),
        ),
        None => select_automatic_source(&destination, &args.manifests, args.include_target)?,
    };
    if source == destination {
        bail!("source and destination workspaces are identical");
    }
    println!(
        "cargo-warm: selected source {} ({source_reason})",
        source.display()
    );

    cache::assert_workspace_quiescent(&source)?;
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
        ) && seen.insert(to.clone())
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

    for path in &args.seed_paths {
        if cache::seed_workspace_path(&source, &destination, path, strategy)? {
            created += 1;
        }
    }

    let build_pairs: Vec<_> = source_paths
        .iter()
        .zip(&destination_paths)
        .filter_map(|(source, destination)| {
            Some((
                source.build_directory.clone()?,
                destination.build_directory.clone()?,
            ))
        })
        .collect();
    if !args.no_freshness_rebase {
        let source_build_dirs: Vec<_> = build_pairs
            .iter()
            .map(|(source_build, _)| source_build.clone())
            .collect();
        match freshness::materializable_link_search_paths(&source, &destination, &source_build_dirs)
        {
            Ok(paths) => {
                for path in paths {
                    match cache::seed_workspace_path(&source, &destination, &path, strategy) {
                        Ok(true) => {
                            created += 1;
                            println!(
                                "cargo-warm: auto-forked build-script native state {}",
                                path.display()
                            );
                        }
                        Ok(false) => {}
                        Err(error) => eprintln!(
                            "cargo-warm: could not auto-fork {}: {error:#}; Cargo will revalidate it normally",
                            path.display()
                        ),
                    }
                }
            }
            Err(error) => eprintln!(
                "cargo-warm: could not inspect build-script native state: {error:#}; continuing with the ordinary cache fork"
            ),
        }

        let package_roots = source_paths
            .iter()
            .flat_map(|paths| paths.package_roots.clone())
            .collect();
        match freshness::synchronize(&source, &destination, &build_pairs, &package_roots) {
            Ok(report) if report.blocking_outputs() == 0 => {
                println!(
                    "cargo-warm: freshness rebased for {} identical tracked file(s); {} build-script directive(s) relocated; {} watched path entries synchronized",
                    report.synced_files,
                    report.rebased_build_script_directives,
                    report.synced_watched_entries
                );
            }
            Ok(report) => {
                eprintln!(
                    "cargo-warm: freshness rebase withheld: {} source-path-sensitive build-script directive(s) are not safely relocatable",
                    report.blocking_outputs()
                );
                eprintln!(
                    "cargo-warm: run `cargo warm doctor` to inspect the blockers; the private 3A cache fork remains usable"
                );
            }
            Err(error) => eprintln!(
                "cargo-warm: freshness rebase was unavailable: {error:#}; continuing with the private 3A cache fork"
            ),
        }
    }

    cache::write_registry(&registry)?;
    if created == 0 {
        println!("cargo-warm: nothing seeded");
    } else {
        println!("cargo-warm: seeded {created} private state item(s)");
    }

    if args.prime {
        println!(
            "cargo-warm: priming destination-native incremental state without changing source bytes"
        );
        let elapsed = prime::run(&destination, &args.manifests, args.unstable_bootstrap)
            .with_context(|| {
                "cache seed succeeded, but relocation prime failed; the seeded 3A/3B state remains safe to use"
            })?;
        println!(
            "cargo-warm: relocation prime completed in {:.2}s",
            elapsed.as_secs_f64()
        );
    }
    Ok(())
}

fn select_automatic_source(
    destination: &std::path::Path,
    manifests: &[std::path::PathBuf],
    include_target: bool,
) -> Result<(std::path::PathBuf, String)> {
    let candidates = git::source_worktree_candidates(destination)?;
    let mut fallback = None;
    for selection in candidates {
        if !cache::workspace_quiescent(&selection.path).unwrap_or(false)
            || !cache::toolchains_compatible(&selection.path, destination).unwrap_or(false)
        {
            continue;
        }
        if fallback.is_none() {
            fallback = Some((selection.path.clone(), selection.reason.clone()));
        }
        let Ok(paths) = cache::resolve_manifests(&selection.path, manifests) else {
            continue;
        };
        let count = cache::seedable_state_count(&paths, include_target);
        if count > 0 {
            return Ok((
                selection.path,
                format!("{}; {count} warm cache root(s)", selection.reason),
            ));
        }
    }

    fallback.ok_or_else(|| {
        anyhow!(
            "could not find a quiescent worktree with a compatible Cargo/rustc toolchain; pass --from explicitly"
        )
    })
}
