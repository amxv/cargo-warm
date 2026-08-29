use std::{collections::BTreeSet, fs, path::PathBuf, thread, time::Instant};

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

#[derive(Debug)]
struct CacheSeedTask {
    kind: CacheKind,
    source: PathBuf,
    destination: PathBuf,
    manifest: PathBuf,
}

pub fn run(args: SeedArgs) -> Result<()> {
    let trace_timings = args.timings || std::env::var_os("CARGO_WARM_TIMINGS").is_some();
    let total_started = Instant::now();
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
            clone_pressure: args.clone_pressure,
            clone_workers: args.clone_workers,
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
    println!(
        "cargo-warm: clone pressure {}{}",
        effective.clone.pressure,
        effective
            .clone
            .workers
            .map(|workers| format!(" (workers={workers})"))
            .unwrap_or_default()
    );
    validate_prime_profile(&destination, profile.prime, profile.unstable_bootstrap)?;

    let preflight_started = Instant::now();
    cache::assert_workspace_quiescent(&destination)?;
    let (source, source_reason, source_paths, destination_paths) = thread::scope(|scope| {
        let destination_paths =
            scope.spawn(|| cache::resolve_manifests(&destination, &effective.manifests));
        let (source, source_reason, pre_resolved_source_paths, source_preflighted) =
            match args.source {
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
        if !source_preflighted {
            cache::assert_workspace_quiescent(&source)?;
            cache::assert_compatible_toolchains(&source, &destination)?;
        }
        let source_paths = match pre_resolved_source_paths {
            Some(paths) => paths,
            None => cache::resolve_manifests(&source, &effective.manifests)?,
        };
        let destination_paths = destination_paths
            .join()
            .map_err(|_| anyhow!("destination Cargo metadata worker panicked"))??;
        Ok::<_, anyhow::Error>((source, source_reason, source_paths, destination_paths))
    })?;
    println!(
        "cargo-warm: selected source {} ({source_reason})",
        source.display()
    );
    if source_paths.len() != destination_paths.len() {
        bail!("source and destination resolved different manifest counts");
    }
    timing(
        trace_timings,
        "source + manifest preflight",
        preflight_started,
    );

    let strategy = cache::clone_strategy(profile.copy_fallback)?;
    let mut registry = cache::read_registry()?;
    let mut seen = BTreeSet::new();
    let mut created = 0usize;
    let mut cache_tasks = Vec::new();
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
    let source_build_dirs: Vec<_> = build_pairs
        .iter()
        .map(|(source_build, _)| source_build.clone())
        .collect();
    let package_roots = source_paths
        .iter()
        .flat_map(|paths| paths.package_roots.clone())
        .collect();

    let clone_started = Instant::now();
    for (source_paths, destination_paths) in source_paths.iter().zip(&destination_paths) {
        if source_paths.manifest.file_name() != destination_paths.manifest.file_name() {
            bail!("source and destination manifest layouts differ");
        }

        if let (Some(from), Some(to)) = (
            source_paths.build_directory.as_ref(),
            destination_paths.build_directory.as_ref(),
        ) && seen.insert(to.clone())
        {
            cache_tasks.push(CacheSeedTask {
                kind: CacheKind::Build,
                source: from.clone(),
                destination: to.clone(),
                manifest: destination_paths.manifest.clone(),
            });
        }

        if profile.include_target || source_paths.build_directory.is_none() {
            let from = &source_paths.target_directory;
            let to = &destination_paths.target_directory;
            if seen.insert(to.clone()) {
                cache_tasks.push(CacheSeedTask {
                    kind: CacheKind::Target,
                    source: from.clone(),
                    destination: to.clone(),
                    manifest: destination_paths.manifest.clone(),
                });
            }
        }
    }
    let clone_workers = effective.clone.effective_workers(cache_tasks.len());
    println!(
        "cargo-warm: cloning {} cache root(s) with {} worker(s)",
        cache_tasks.len(),
        clone_workers
    );
    let (records, build_output_plan) = thread::scope(|scope| -> Result<_> {
        let clone_handle = scope.spawn(|| {
            seed_cache_roots_parallel(&cache_tasks, clone_workers, &source, &destination, strategy)
        });
        let plan_result = profile
            .freshness_rebase
            .then(|| freshness::prepare_build_outputs(&source, &destination, &source_build_dirs))
            .transpose();
        let records_result = clone_handle
            .join()
            .map_err(|_| anyhow!("cache clone coordinator panicked"))?;

        match (records_result, plan_result) {
            (Ok(records), Ok(plan)) => Ok((records, plan)),
            (Ok(records), Err(error)) => {
                rollback_seed_records(&records);
                Err(error)
            }
            (Err(error), _) => Err(error),
        }
    })?;
    for record in records {
        registry
            .records
            .retain(|existing| existing.path != record.path);
        registry.records.push(record);
        created += 1;
    }
    timing(
        trace_timings,
        "Cargo cache clone + freshness planning",
        clone_started,
    );

    for path in &profile.seed_paths {
        if cache::seed_workspace_path(&source, &destination, path, strategy)? {
            created += 1;
        }
    }

    if profile.freshness_rebase {
        let native_scan_started = Instant::now();
        if let Some(plan) = &build_output_plan {
            for path in &plan.materializable_paths {
                match cache::seed_workspace_path(&source, &destination, path, strategy) {
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
        timing(
            trace_timings,
            "native state discovery + fork",
            native_scan_started,
        );

        let freshness_started = Instant::now();
        let path_sensitive_outputs = build_output_plan
            .map(|plan| plan.path_sensitive_outputs)
            .unwrap_or_default();
        match freshness::synchronize_prepared(
            &source,
            &destination,
            &build_pairs,
            &package_roots,
            path_sensitive_outputs,
        ) {
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
        timing(
            trace_timings,
            "freshness synchronization",
            freshness_started,
        );
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
    timing(trace_timings, "total seed", total_started);
    Ok(())
}

fn seed_cache_roots_parallel(
    tasks: &[CacheSeedTask],
    workers: usize,
    source_workspace: &std::path::Path,
    destination_workspace: &std::path::Path,
    strategy: cache::CloneStrategy,
) -> Result<Vec<cache::SeedRecord>> {
    let mut created = Vec::new();
    for chunk in tasks.chunks(workers.max(1)) {
        let results = thread::scope(|scope| {
            let mut handles = Vec::with_capacity(chunk.len());
            for task in chunk {
                handles.push(scope.spawn(move || {
                    cache::seed_one(
                        task.kind,
                        &task.source,
                        &task.destination,
                        source_workspace,
                        destination_workspace,
                        &task.manifest,
                        strategy,
                    )
                }));
            }
            let mut records = Vec::new();
            let mut first_error = None;
            for handle in handles {
                match handle.join() {
                    Ok(Ok(Some(record))) => records.push(record),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                    Err(_) => {
                        if first_error.is_none() {
                            first_error = Some(anyhow!("cache clone worker panicked"));
                        }
                    }
                }
            }
            (records, first_error)
        });

        let (records, error) = results;
        created.extend(records);
        if let Some(error) = error {
            rollback_seed_records(&created);
            return Err(error);
        }
    }
    Ok(created)
}

fn rollback_seed_records(records: &[cache::SeedRecord]) {
    for record in records {
        let _ = fs::remove_dir_all(&record.path);
    }
}

fn timing(enabled: bool, label: &str, started: Instant) {
    if enabled {
        eprintln!(
            "cargo-warm timing: {label}: {:.3}s",
            started.elapsed().as_secs_f64()
        );
    }
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
    if let Some(source) = select_from_candidates(
        destination,
        manifests,
        include_target,
        git::exact_worktree_candidates(destination)?,
    )? {
        return Ok(source);
    }

    if let Some(source) = select_from_candidates(
        destination,
        manifests,
        include_target,
        git::nearby_worktree_candidates(destination)?,
    )? {
        return Ok(source);
    }

    Err(anyhow!(
        "could not find a quiescent compatible worktree with seedable warm Cargo state; warm another checkout first or pass --from explicitly"
    ))
}

fn select_from_candidates(
    destination: &std::path::Path,
    manifests: &[std::path::PathBuf],
    include_target: bool,
    candidates: Vec<git::SourceSelection>,
) -> Result<Option<AutomaticSource>> {
    for selection in candidates {
        if !cache::workspace_quiescent(&selection.path).unwrap_or(false)
            || !cache::toolchains_compatible(&selection.path, destination).unwrap_or(false)
        {
            continue;
        }
        let Ok(paths) = cache::resolve_manifests(&selection.path, manifests) else {
            continue;
        };
        let count = cache::seedable_state_count(&paths, include_target);
        if count > 0 {
            return Ok(Some(AutomaticSource {
                path: selection.path,
                reason: format!("{}; {count} warm cache root(s)", selection.reason),
                paths,
            }));
        }
    }
    Ok(None)
}
