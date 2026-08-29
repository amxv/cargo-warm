use std::collections::BTreeSet;

use anyhow::{Context, Result, anyhow, bail};

use crate::{
    cache::{self, CacheKind, CargoPaths},
    cli::SeedArgs,
    compiler::{RustcChannel, rustc_info},
    config::{self, PrimeMode, SeedOverrides},
    freshness, git, prime,
};

pub(crate) struct AutomaticSource {
    pub(crate) path: std::path::PathBuf,
    pub(crate) reason: String,
    pub(crate) paths: Vec<CargoPaths>,
}

pub fn run(args: SeedArgs) -> Result<()> {
    let destination = cache::canonical_dir(&args.destination)?;
    let effective = config::resolve_seed(
        &destination,
        SeedOverrides {
            profile: args.profile.clone(),
            config: args.config.clone(),
            manifests: args.manifests.clone(),
            include_target: args.include_target,
            copy_fallback: args.copy_fallback,
            seed_paths: args.seed_paths.clone(),
            disable_freshness_rebase: args.no_freshness_rebase,
            legacy_prime: args.prime,
            prime_mode: args.prime_mode,
            unstable_bootstrap: args.unstable_bootstrap,
        },
    )?;
    let profile = &effective.profile;
    println!(
        "cargo-warm: profile {} (prime={}, freshness={}, include-target={})",
        profile.name,
        profile.prime,
        if profile.freshness_rebase {
            "on"
        } else {
            "off"
        },
        if profile.include_target { "yes" } else { "no" }
    );
    if let Some(path) = &effective.config_path {
        println!("cargo-warm: config {}", path.display());
    }
    validate_prime_profile(&destination, profile.prime, profile.unstable_bootstrap)?;

    cache::assert_workspace_quiescent(&destination)?;
    let (source, source_reason, pre_resolved_source_paths, source_preflighted) = match args.source {
        Some(source) => (
            cache::canonical_dir(&source)?,
            "explicit --from".to_string(),
            None,
            false,
        ),
        None => {
            let selection = select_automatic_source(
                &destination,
                &effective.manifests,
                profile.include_target,
            )?;
            (
                selection.path,
                selection.reason,
                Some(selection.paths),
                true,
            )
        }
    };
    if source == destination {
        bail!("source and destination workspaces are identical");
    }
    println!(
        "cargo-warm: selected source {} ({source_reason})",
        source.display()
    );

    if !source_preflighted {
        cache::assert_workspace_quiescent(&source)?;
        cache::assert_compatible_toolchains(&source, &destination)?;
    }
    let source_paths = match pre_resolved_source_paths {
        Some(paths) => paths,
        None => cache::resolve_manifests(&source, &effective.manifests)?,
    };
    let destination_paths = cache::resolve_manifests(&destination, &effective.manifests)?;
    if source_paths.len() != destination_paths.len() {
        bail!("source and destination resolved different manifest counts");
    }

    let strategy = cache::clone_strategy(profile.copy_fallback)?;
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

        if profile.include_target || source_paths.build_directory.is_none() {
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

    for path in &profile.seed_paths {
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
    if profile.freshness_rebase {
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
                    "cargo-warm: run `cargo warm doctor` to inspect the blockers; the private cache fork remains usable"
                );
            }
            Err(error) => eprintln!(
                "cargo-warm: freshness rebase was unavailable: {error:#}; continuing with the private cache fork"
            ),
        }
    }

    cache::write_registry(&registry)?;
    if created == 0 {
        println!("cargo-warm: nothing seeded");
    } else {
        println!("cargo-warm: seeded {created} private state item(s)");
    }

    if profile.prime != PrimeMode::None {
        println!(
            "cargo-warm: {} prime: preparing destination-native incremental state without changing source bytes",
            profile.prime
        );
        let elapsed = prime::run(
            &destination,
            &effective.manifests,
            profile.prime,
            profile.unstable_bootstrap,
        )
            .with_context(|| {
                "cache seed succeeded, but relocation prime failed; the seeded private cache remains safe to use"
            })?;
        println!(
            "cargo-warm: relocation prime completed in {:.2}s",
            elapsed.as_secs_f64()
        );
    }
    Ok(())
}

fn validate_prime_profile(
    workspace: &std::path::Path,
    prime: PrimeMode,
    unstable_bootstrap: bool,
) -> Result<()> {
    if prime == PrimeMode::None {
        return Ok(());
    }
    let rustc = rustc_info(workspace)?;
    if !rustc.relocatable_incremental_supported {
        bail!(
            "profile requests {prime} priming, but rustc {} predates relocatable incremental support; use profile `quick` or Rust 1.98+",
            rustc.release
        );
    }
    if matches!(rustc.channel, RustcChannel::Stable | RustcChannel::Beta) && !unstable_bootstrap {
        bail!(
            "profile requests {prime} priming on rustc {}; pass --unstable-bootstrap or set `unstable-bootstrap = true` in .agents/.cargo-warm.toml",
            rustc.release
        );
    }
    Ok(())
}

pub(crate) fn select_automatic_source(
    destination: &std::path::Path,
    manifests: &[std::path::PathBuf],
    include_target: bool,
) -> Result<AutomaticSource> {
    let candidates = git::source_worktree_candidates(destination)?;
    let mut fallback = None;
    for selection in candidates {
        if !cache::workspace_quiescent(&selection.path).unwrap_or(false)
            || !cache::toolchains_compatible(&selection.path, destination).unwrap_or(false)
        {
            continue;
        }
        let Ok(paths) = cache::resolve_manifests(&selection.path, manifests) else {
            continue;
        };
        if fallback.is_none() {
            fallback = Some(AutomaticSource {
                path: selection.path.clone(),
                reason: selection.reason.clone(),
                paths: paths.clone(),
            });
        }
        let count = cache::seedable_state_count(&paths, include_target);
        if count > 0 {
            return Ok(AutomaticSource {
                path: selection.path,
                reason: format!("{}; {count} warm cache root(s)", selection.reason),
                paths,
            });
        }
    }

    fallback.ok_or_else(|| {
        anyhow!(
            "could not find a quiescent worktree with a compatible Cargo/rustc toolchain; pass --from explicitly"
        )
    })
}
