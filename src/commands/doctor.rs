use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cache,
    cli::DoctorArgs,
    config::{self, EffectiveSeedConfig, SeedOverrides},
    freshness,
    project::{self, ProjectShape},
};

#[derive(Debug, Serialize)]
struct DoctorReport {
    config_path: Option<PathBuf>,
    active_profile: config::SeedProfile,
    available_profiles: Vec<String>,
    recommendation: ProfileRecommendation,
    project: ProjectShape,
    comparison_available: bool,
    source: PathBuf,
    destination: PathBuf,
    source_reason: String,
    source_head: Option<String>,
    destination_head: Option<String>,
    same_revision: bool,
    source_clean: bool,
    destination_clean: bool,
    tracked_mtimes: Option<TrackedMtimeSummary>,
    manifests: Vec<ManifestReport>,
    freshness: freshness::FreshnessReport,
    auto_materializable_paths: Vec<PathBuf>,
    probes: Vec<ProbeReport>,
}

#[derive(Debug, Serialize)]
struct ProfileRecommendation {
    profile: String,
    reasons: Vec<String>,
    cautions: Vec<String>,
    benchmark_profiles: Vec<String>,
    config_example: String,
}

#[derive(Debug, Serialize)]
struct TrackedMtimeSummary {
    tracked_files: usize,
    destination_newer: usize,
    same_mtime: usize,
    source_newer: usize,
    missing: usize,
}

#[derive(Debug, Serialize)]
struct ManifestReport {
    manifest: PathBuf,
    source_build_directory: Option<PathBuf>,
    destination_build_directory: Option<PathBuf>,
    destination_target_directory: PathBuf,
    separate_build_directory: bool,
    source_cache_exists: bool,
    destination_cache_exists: bool,
    source_cache_health: Option<CacheHealth>,
    destination_cache_health: Option<CacheHealth>,
    build_scripts: Vec<BuildScriptReport>,
}

#[derive(Debug, Clone, Serialize)]
struct CacheHealth {
    deps_entries: usize,
    incremental_entries: usize,
}

#[derive(Debug, Serialize)]
struct BuildScriptReport {
    package: String,
    path: PathBuf,
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    manifest: PathBuf,
    elapsed_ms: u128,
    cargo_success: bool,
    reasons: Vec<FingerprintReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FingerprintReason {
    package: Option<String>,
    category: FingerprintCategory,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum FingerprintCategory {
    ChangedFile,
    BuildScriptPathsChanged,
    DependencyChanged,
    EnvironmentChanged,
    CompilerChanged,
    FilesystemOutdated,
    Other,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    let destination = cache::canonical_dir(&args.destination)?;
    let effective = config::resolve_seed(
        &destination,
        SeedOverrides {
            profile: args.profile.clone(),
            config: args.config.clone(),
            manifests: args.manifests.clone(),
            ..SeedOverrides::default()
        },
    )?;
    let (
        source,
        source_reason,
        pre_resolved_source_paths,
        source_preflighted,
        comparison_available,
    ) = match args.source {
        Some(source) => (
            cache::canonical_dir(&source)?,
            "explicit --from".to_string(),
            None,
            false,
            true,
        ),
        None => {
            match super::seed::select_automatic_source(
                &destination,
                &effective.manifests,
                effective.profile.include_target,
            ) {
                Ok(selection) => (
                    selection.path,
                    selection.reason,
                    Some(selection.paths),
                    true,
                    true,
                ),
                Err(_error) => (
                    destination.clone(),
                    "project-only analysis; no compatible warm peer was available".to_string(),
                    None,
                    true,
                    false,
                ),
            }
        }
    };

    let source_head = git_head(&source)?;
    let destination_head = git_head(&destination)?;
    let source_clean = git_clean(&source)?;
    let destination_clean = git_clean(&destination)?;
    let same_revision =
        comparison_available && source_head.is_some() && source_head == destination_head;
    let tracked_mtimes =
        if comparison_available && same_revision && source_clean && destination_clean {
            Some(compare_tracked_mtimes(&source, &destination)?)
        } else {
            None
        };

    if comparison_available && !source_preflighted {
        cache::assert_compatible_toolchains(&source, &destination)?;
    }
    let source_paths = match pre_resolved_source_paths {
        Some(paths) => paths,
        None => cache::resolve_manifests(&source, &effective.manifests)?,
    };
    let destination_paths = if comparison_available {
        cache::resolve_manifests(&destination, &effective.manifests)?
    } else {
        source_paths.clone()
    };
    if source_paths.len() != destination_paths.len() {
        return Err(anyhow!(
            "source and destination resolved different manifest counts"
        ));
    }
    let source_build_dirs: Vec<_> = source_paths
        .iter()
        .filter_map(|paths| paths.build_directory.clone())
        .collect();
    let freshness = if comparison_available {
        freshness::analyze_fast(&source, &destination, &source_build_dirs)?
    } else {
        freshness::FreshnessReport::default()
    };
    let auto_materializable_paths = if comparison_available {
        freshness::materializable_link_search_paths(&source, &destination, &source_build_dirs)?
    } else {
        Vec::new()
    };
    let project = project::inspect(&destination, &effective.manifests)?;
    let available_profiles = config::available_profile_names(&destination, args.config.as_deref())?;

    let mut manifests = Vec::with_capacity(destination_paths.len());
    for (source_paths, destination_paths) in source_paths.iter().zip(&destination_paths) {
        let build_scripts = build_scripts(&destination, &destination_paths.manifest)?;
        let source_cache_health = source_paths
            .build_directory
            .as_ref()
            .filter(|path| path.is_dir())
            .map(|path| cache_health(path))
            .transpose()?;
        let destination_cache_health = destination_paths
            .build_directory
            .as_ref()
            .filter(|path| path.is_dir())
            .map(|path| cache_health(path))
            .transpose()?;
        manifests.push(ManifestReport {
            manifest: destination_paths.manifest.clone(),
            source_build_directory: source_paths.build_directory.clone(),
            destination_build_directory: destination_paths.build_directory.clone(),
            destination_target_directory: destination_paths.target_directory.clone(),
            separate_build_directory: destination_paths
                .build_directory
                .as_ref()
                .is_some_and(|path| path != &destination_paths.target_directory),
            source_cache_exists: source_paths
                .build_directory
                .as_ref()
                .is_some_and(|path| path.is_dir()),
            destination_cache_exists: destination_paths
                .build_directory
                .as_ref()
                .is_some_and(|path| path.is_dir()),
            source_cache_health,
            destination_cache_health,
            build_scripts,
        });
    }
    let recommendation = recommend_profile(
        &project,
        &freshness,
        &auto_materializable_paths,
        &manifests,
        &effective,
    );

    let probes = if args.probe {
        let mut probes = Vec::with_capacity(destination_paths.len());
        for paths in &destination_paths {
            probes.push(probe_manifest(
                &destination,
                &paths.manifest,
                &effective.profile,
                effective.config_path.as_deref(),
            )?);
        }
        probes
    } else {
        Vec::new()
    };

    let report = DoctorReport {
        config_path: effective.config_path.clone(),
        active_profile: effective.profile.clone(),
        available_profiles,
        recommendation,
        project,
        comparison_available,
        source,
        destination,
        source_reason,
        source_head,
        destination_head,
        same_revision,
        source_clean,
        destination_clean,
        tracked_mtimes,
        manifests,
        freshness,
        auto_materializable_paths,
        probes,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, args.probe);
    }
    Ok(())
}

fn recommend_profile(
    project: &ProjectShape,
    freshness: &freshness::FreshnessReport,
    auto_materializable_paths: &[PathBuf],
    manifests: &[ManifestReport],
    effective: &EffectiveSeedConfig,
) -> ProfileRecommendation {
    let mut reasons = Vec::new();
    let mut cautions = Vec::new();
    let mut benchmark_profiles = Vec::new();

    let profile = if !project.rustc.relocatable_incremental_supported {
        reasons.push(format!(
            "rustc {} predates relocatable incremental support, so compiler priming is unavailable",
            project.rustc.release
        ));
        "quick"
    } else if project.rust_lines < 20_000 && project.rust_source_files < 80 {
        reasons.push(format!(
            "the selected local package set is relatively small ({} Rust lines across {} files), so provisioning latency is likely more valuable than an eager prime",
            project.rust_lines, project.rust_source_files
        ));
        benchmark_profiles.push("balanced".to_string());
        "quick"
    } else if (project.rust_lines >= 100_000 || project.rust_source_files >= 300)
        && project.direct_build_scripts > 0
    {
        reasons.push(format!(
            "the selected package set is large ({} Rust lines across {} files) and has {} direct build script(s); large crates can retain a first-edit tax until the package build boundary is re-established",
            project.rust_lines, project.rust_source_files, project.direct_build_scripts
        ));
        benchmark_profiles.extend(["balanced".to_string(), "deep".to_string()]);
        "deep"
    } else {
        reasons.push(format!(
            "the selected package set is large enough to benefit from a relocatable rustc prime ({} Rust lines across {} files)",
            project.rust_lines, project.rust_source_files
        ));
        benchmark_profiles.push("quick".to_string());
        if project.direct_build_scripts > 0 {
            benchmark_profiles.push("deep".to_string());
            cautions.push(
                "`deep` intentionally makes the selected package's own build script stale during provisioning; benchmark it before making it the default if that build script is expensive"
                    .to_string(),
            );
        }
        "balanced"
    };

    if !auto_materializable_paths.is_empty() {
        reasons.push(format!(
            "{} ignored native link artifact/path(s) can be cloned automatically instead of rebuilding their whole native toolchain",
            auto_materializable_paths.len()
        ));
    }
    if freshness.blocking_outputs() > 0 {
        if auto_materializable_paths.is_empty() {
            cautions.push(format!(
                "{} cached build-script path directive(s) cannot currently be relocated safely; provide equivalent destination state or an explicit seed path before expecting full freshness",
                freshness.blocking_outputs()
            ));
        } else {
            cautions.push(format!(
                "{} cached build-script path directive(s) currently reference missing destination state; seed can auto-materialize {} final native artifact/path(s) before freshness is re-evaluated",
                freshness.blocking_outputs(),
                auto_materializable_paths.len()
            ));
        }
    }
    if project.other_local_build_scripts > 0 {
        cautions.push(format!(
            "{} other local/path-dependency build script(s) were detected; package priming deliberately leaves them untouched",
            project.other_local_build_scripts
        ));
    }
    let shared_target_build_dirs = manifests
        .iter()
        .filter(|manifest| !manifest.separate_build_directory)
        .count();
    if shared_target_build_dirs > 0 {
        cautions.push(format!(
            "{shared_target_build_dirs} manifest(s) still use the same directory for intermediate build state and final target artifacts; Cargo 1.91+ can separate them with `build.build-dir`, which gives cargo-warm a smaller per-worktree cache boundary"
        ));
    }
    let source_deps_entries: usize = manifests
        .iter()
        .filter_map(|manifest| manifest.source_cache_health.as_ref())
        .map(|health| health.deps_entries)
        .sum();
    if source_deps_entries >= 20_000 {
        cautions.push(format!(
            "the warm source cache contains {source_deps_entries} entries across deps directories; very large stale deps sets can slow Cargo itself, so benchmark after pruning obsolete build families as well as after changing profiles"
        ));
    }
    if matches!(
        project.rustc.channel,
        crate::compiler::RustcChannel::Stable | crate::compiler::RustcChannel::Beta
    ) && profile != "quick"
        && !effective.profile.unstable_bootstrap
    {
        cautions.push(
            "stable/beta Rust still requires explicit `--unstable-bootstrap` (or `unstable-bootstrap = true` in project config) for relocatable priming"
                .to_string(),
        );
    }
    benchmark_profiles.retain(|candidate| candidate != profile);
    benchmark_profiles.dedup();

    ProfileRecommendation {
        profile: profile.to_string(),
        reasons,
        cautions,
        benchmark_profiles,
        config_example: recommended_config_example(profile, project, &effective.manifests),
    }
}

fn cache_health(root: &Path) -> Result<CacheHealth> {
    let mut health = CacheHealth {
        deps_entries: 0,
        incremental_entries: 0,
    };
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        if depth > 3 {
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let name = directory.file_name().and_then(|value| value.to_str());
        if name == Some("deps") {
            health.deps_entries += entries.count();
            continue;
        }
        if name == Some("incremental") {
            health.incremental_entries += entries.count();
            continue;
        }
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    Ok(health)
}

fn recommended_config_example(
    profile: &str,
    project: &ProjectShape,
    manifests: &[PathBuf],
) -> String {
    let manifests = manifests
        .iter()
        .map(|path| format!("\"{}\"", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut text =
        format!("version = 1\ndefault-profile = \"{profile}\"\nmanifests = [{manifests}]\n");
    if profile != "quick"
        && matches!(
            project.rustc.channel,
            crate::compiler::RustcChannel::Stable | crate::compiler::RustcChannel::Beta
        )
    {
        text.push_str("unstable-bootstrap = true\n");
    }
    text
}

fn git_head(workspace: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(["rev-parse", "HEAD"])
        .output()
        .with_context(|| format!("failed to inspect Git revision in {}", workspace.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

fn git_clean(workspace: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .with_context(|| format!("failed to inspect Git status in {}", workspace.display()))?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(output.stdout.is_empty())
}

fn compare_tracked_mtimes(source: &Path, destination: &Path) -> Result<TrackedMtimeSummary> {
    let output = Command::new("git")
        .current_dir(destination)
        .args(["ls-files", "-z"])
        .output()
        .with_context(|| "failed to enumerate tracked files")?;
    if !output.status.success() {
        return Err(anyhow!("git ls-files failed in {}", destination.display()));
    }

    let mut summary = TrackedMtimeSummary {
        tracked_files: 0,
        destination_newer: 0,
        same_mtime: 0,
        source_newer: 0,
        missing: 0,
    };
    for raw_path in output.stdout.split(|byte| *byte == 0) {
        if raw_path.is_empty() {
            continue;
        }
        summary.tracked_files += 1;
        let relative = PathBuf::from(String::from_utf8_lossy(raw_path).into_owned());
        let source_mtime =
            fs::symlink_metadata(source.join(&relative)).and_then(|metadata| metadata.modified());
        let destination_mtime = fs::symlink_metadata(destination.join(&relative))
            .and_then(|metadata| metadata.modified());
        let (Ok(source_mtime), Ok(destination_mtime)) = (source_mtime, destination_mtime) else {
            summary.missing += 1;
            continue;
        };
        if destination_mtime > source_mtime {
            summary.destination_newer += 1;
        } else if source_mtime > destination_mtime {
            summary.source_newer += 1;
        } else {
            summary.same_mtime += 1;
        }
    }
    Ok(summary)
}

fn build_scripts(workspace: &Path, manifest: &Path) -> Result<Vec<BuildScriptReport>> {
    let output = Command::new("cargo")
        .current_dir(workspace)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .with_context(|| {
            format!(
                "failed to inspect Cargo metadata for {}",
                manifest.display()
            )
        })?;
    if !output.status.success() {
        return Err(anyhow!("cargo metadata failed for {}", manifest.display()));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let mut scripts = Vec::new();
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("cargo metadata did not report packages"))?;
    for package in packages {
        let package_name = package
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let Some(targets) = package.get("targets").and_then(Value::as_array) else {
            continue;
        };
        for target in targets {
            let is_build_script =
                target
                    .get("kind")
                    .and_then(Value::as_array)
                    .is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|kind| kind.as_str() == Some("custom-build"))
                    });
            if !is_build_script {
                continue;
            }
            if let Some(path) = target.get("src_path").and_then(Value::as_str) {
                scripts.push(BuildScriptReport {
                    package: package_name.to_owned(),
                    path: PathBuf::from(path),
                });
            }
        }
    }
    Ok(scripts)
}

fn probe_manifest(
    workspace: &Path,
    manifest: &Path,
    profile: &config::SeedProfile,
    config_path: Option<&Path>,
) -> Result<ProbeReport> {
    let started = Instant::now();
    let mut command = if profile.prime == config::PrimeMode::None {
        let mut command = Command::new("cargo");
        command.arg("check");
        command
    } else {
        let current_exe =
            std::env::current_exe().context("failed to resolve cargo-warm executable")?;
        let mut command = Command::new(current_exe);
        command.arg("check").arg("--profile").arg(&profile.name);
        if let Some(config_path) = config_path {
            command.arg("--config").arg(config_path);
        }
        command
    };
    let output = command
        .current_dir(workspace)
        .env("CARGO_LOG", "cargo::core::compiler::fingerprint=info")
        .args(["--manifest-path"])
        .arg(manifest)
        .stdout(Stdio::null())
        .output()
        .with_context(|| {
            format!(
                "failed to probe Cargo fingerprints for {}",
                manifest.display()
            )
        })?;
    let reasons = parse_fingerprint_reasons(&String::from_utf8_lossy(&output.stderr));
    Ok(ProbeReport {
        manifest: manifest.to_path_buf(),
        elapsed_ms: started.elapsed().as_millis(),
        cargo_success: output.status.success(),
        reasons,
    })
}

fn parse_fingerprint_reasons(log: &str) -> Vec<FingerprintReason> {
    let mut reasons = BTreeSet::new();
    for line in log.lines() {
        if !line.contains("cargo::core::compiler::fingerprint") {
            continue;
        }
        let package = extract_package(line);
        let trimmed = line.trim();
        let category = if trimmed.contains("stale: changed ") {
            FingerprintCategory::ChangedFile
        } else if trimmed.contains("RerunIfChangedOutputPathsChanged") {
            FingerprintCategory::BuildScriptPathsChanged
        } else if trimmed.contains("UnitDependencyInfoChanged")
            || trimmed.contains("StaleDepFingerprint")
            || trimmed.contains("unit dependency information changed")
        {
            FingerprintCategory::DependencyChanged
        } else if trimmed.contains("EnvVar") || trimmed.contains("environment variable") {
            FingerprintCategory::EnvironmentChanged
        } else if trimmed.contains("path to the compiler has changed") {
            FingerprintCategory::CompilerChanged
        } else if trimmed.contains("FsStatusOutdated")
            || trimmed.contains("current filesystem status shows we're outdated")
        {
            FingerprintCategory::FilesystemOutdated
        } else if trimmed.contains("dirty:") || trimmed.contains("err:") {
            FingerprintCategory::Other
        } else {
            continue;
        };
        let detail = trimmed
            .split("cargo::core::compiler::fingerprint:")
            .nth(1)
            .unwrap_or(trimmed)
            .trim()
            .to_owned();
        reasons.insert(FingerprintReason {
            package,
            category,
            detail,
        });
    }
    reasons.into_iter().collect()
}

fn extract_package(line: &str) -> Option<String> {
    let start = line.find("package_id=")? + "package_id=".len();
    let rest = &line[start..];
    let end = rest.find(" target=").unwrap_or(rest.len());
    Some(rest[..end].trim_matches('"').to_owned())
}

fn print_human_report(report: &DoctorReport, probed: bool) {
    println!("cargo-warm doctor");
    println!(
        "  config:      {}",
        report
            .config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none (built-in defaults)".to_string())
    );
    println!(
        "  profile:     {} (prime={}, freshness={}, include-target={})",
        report.active_profile.name,
        report.active_profile.prime,
        if report.active_profile.freshness_rebase {
            "on"
        } else {
            "off"
        },
        if report.active_profile.include_target {
            "yes"
        } else {
            "no"
        }
    );
    println!("  profiles:    {}", report.available_profiles.join(", "));
    if report.comparison_available {
        println!(
            "  source:      {} ({})",
            report.source.display(),
            report.source_reason
        );
        println!("  destination: {}", report.destination.display());
        println!(
            "  revision:    {}",
            if report.same_revision {
                "exact Git revision"
            } else {
                "different Git revisions"
            }
        );
        println!(
            "  worktrees:   source={} destination={}",
            clean_label(report.source_clean),
            clean_label(report.destination_clean)
        );
    } else {
        println!("  project:     {}", report.destination.display());
        println!("  comparison:  {}", report.source_reason);
    }

    if let Some(mtimes) = &report.tracked_mtimes {
        println!(
            "  mtimes:      {} tracked, {} newer in destination, {} identical",
            mtimes.tracked_files, mtimes.destination_newer, mtimes.same_mtime
        );
        if mtimes.destination_newer > 0 {
            println!(
                "  note:        identical Git worktrees can still look stale to Cargo when checkout mtimes are newer than cloned fingerprints"
            );
        }
    } else if report.comparison_available {
        println!("  mtimes:      skipped (requires clean worktrees at the same revision)");
    }

    if report.comparison_available {
        println!(
            "  freshness:   {} identical tracked file(s) eligible for safe mtime synchronization",
            report.freshness.eligible_files
        );
    }
    if report.comparison_available && report.freshness.path_sensitive_outputs.is_empty() {
        println!("  path safety: no source-worktree paths found in cached build-script directives");
    } else if report.comparison_available {
        println!(
            "  path safety: {} rebasable, {} blocking build-script directive(s)",
            report.freshness.rebasable_outputs(),
            report.freshness.blocking_outputs()
        );
        let mut shown = BTreeSet::new();
        for output in report
            .freshness
            .path_sensitive_outputs
            .iter()
            .filter(|output| shown.insert((output.command.clone(), output.reason.clone())))
            .take(8)
        {
            let state = if output.rebasable {
                "rebasable"
            } else {
                "blocking"
            };
            println!("    [{state}] {}", output.command);
            if let Some(reason) = &output.reason {
                println!("      {reason}");
            }
        }
        if report.freshness.blocking_outputs() > 0 {
            if report.auto_materializable_paths.is_empty() {
                println!(
                    "  note:        cargo warm seed withholds mtime rebasing until blockers have equivalent destination state; --seed-path can provide unusual native state explicitly"
                );
            } else {
                println!(
                    "  auto-fork:   {} ignored final native artifact/path(s) can be materialized safely by cargo warm seed",
                    report.auto_materializable_paths.len()
                );
                for path in report.auto_materializable_paths.iter().take(8) {
                    println!("    {}", path.display());
                }
            }
        }
    }

    for manifest in &report.manifests {
        println!("\nmanifest: {}", manifest.manifest.display());
        match &manifest.destination_build_directory {
            Some(path) => println!(
                "  build cache: {} ({})",
                path.display(),
                if manifest.destination_cache_exists {
                    "present"
                } else {
                    "missing"
                }
            ),
            None => println!("  build cache: Cargo did not report a separate build_directory"),
        }
        println!(
            "  target dir:   {}",
            manifest.destination_target_directory.display()
        );
        if !manifest.separate_build_directory {
            println!(
                "  tip: Cargo 1.91+ can keep intermediate state separate with `build.build-dir`; for worktrees, a useful starting point is `build-dir = \"{{cargo-cache-home}}/build/{{workspace-path-hash}}\"`"
            );
        }
        if let Some(health) = &manifest.source_cache_health {
            println!(
                "  {}cache: {} deps entries, {} incremental entries",
                if report.comparison_available {
                    "warm source "
                } else {
                    "current "
                },
                health.deps_entries,
                health.incremental_entries
            );
        }
        if report.comparison_available
            && let Some(health) = &manifest.destination_cache_health
        {
            println!(
                "  destination cache: {} deps entries, {} incremental entries",
                health.deps_entries, health.incremental_entries
            );
        }
        if manifest.build_scripts.is_empty() {
            println!("  build scripts: none in workspace packages");
        } else {
            println!("  build scripts: {}", manifest.build_scripts.len());
            for script in &manifest.build_scripts {
                println!("    {}: {}", script.package, script.path.display());
            }
            println!(
                "  note: build scripts can invalidate on watched inputs and may cache checkout-local paths"
            );
        }
    }

    println!("\nproject shape");
    println!(
        "  rustc:       {} ({:?})",
        report.project.rustc.release, report.project.rustc.channel
    );
    println!(
        "  local Rust:  {} lines across {} source files",
        report.project.rust_lines, report.project.rust_source_files
    );
    println!(
        "  build scripts: {} selected package(s), {} other local/path package(s)",
        report.project.direct_build_scripts, report.project.other_local_build_scripts
    );
    for manifest in &report.project.manifests {
        for package in &manifest.selected_packages {
            println!(
                "    {}: {} lines / {} files{}",
                package.name,
                package.rust_lines,
                package.rust_source_files,
                if package.has_build_script {
                    " / build.rs"
                } else {
                    ""
                }
            );
        }
    }

    println!("\nrecommendation");
    println!("  profile: {}", report.recommendation.profile);
    for reason in &report.recommendation.reasons {
        println!("  why:     {reason}");
    }
    for caution in &report.recommendation.cautions {
        println!("  caution: {caution}");
    }
    if !report.recommendation.benchmark_profiles.is_empty() {
        println!(
            "  compare:  {}",
            report.recommendation.benchmark_profiles.join(", ")
        );
        println!(
            "  method:   measure worktree creation separately from the first representative Rust edit; keep the same warm source checkout and toolchain for every profile"
        );
        println!(
            "  commands: cargo warm seed --profile {}",
            report.recommendation.profile
        );
        for profile in &report.recommendation.benchmark_profiles {
            println!("            cargo warm seed --profile {profile}");
        }
    }
    println!("\nSuggested .agents/.cargo-warm.toml:\n");
    print!("{}", report.recommendation.config_example);
    let recommended_prime = match report.recommendation.profile.as_str() {
        "quick" => crate::config::PrimeMode::None,
        "balanced" => crate::config::PrimeMode::Rustc,
        "deep" => crate::config::PrimeMode::Package,
        _ => report.active_profile.prime,
    };
    if report.active_profile.prime != recommended_prime {
        println!(
            "\nCurrent profile `{}` differs from the static recommendation `{}`. Treat the recommendation as a starting point; measured first-edit latency wins.",
            report.active_profile.name, report.recommendation.profile
        );
    }

    if !probed {
        println!(
            "\nRun `cargo warm doctor --probe` to execute cargo check and classify Cargo's actual fingerprint misses."
        );
        return;
    }

    for probe in &report.probes {
        println!("\nprobe: {}", probe.manifest.display());
        println!(
            "  cargo check: {} in {:.2}s",
            if probe.cargo_success {
                "passed"
            } else {
                "failed"
            },
            probe.elapsed_ms as f64 / 1000.0
        );
        if probe.reasons.is_empty() {
            println!("  fingerprint misses: none reported");
        } else {
            println!("  fingerprint misses: {}", probe.reasons.len());
            for reason in &probe.reasons {
                println!(
                    "    {:?}: {}{}",
                    reason.category,
                    reason
                        .package
                        .as_deref()
                        .map(|package| format!("{package}: "))
                        .unwrap_or_default(),
                    reason.detail
                );
            }
        }
    }
}

fn clean_label(clean: bool) -> &'static str {
    if clean { "clean" } else { "dirty" }
}

#[cfg(test)]
mod tests {
    use super::{FingerprintCategory, parse_fingerprint_reasons};

    #[test]
    fn classifies_current_cargo_fingerprint_reasons() {
        let log = r#"
INFO prepare_target{force=false package_id=example-app v0.6.4 (/tmp/wt/app) target="example_app"}: cargo::core::compiler::fingerprint: stale: changed "/tmp/wt/app/build.rs"
INFO prepare_target{force=false package_id=example-app v0.6.4 (/tmp/wt/app) target="build-script-build"}: cargo::core::compiler::fingerprint:     dirty: RerunIfChangedOutputPathsChanged { old: ["build.rs"], new: ["/tmp/wt/build.rs"] }
INFO prepare_target{force=false package_id=example-app v0.6.4 (/tmp/wt/app) target="example_app"}: cargo::core::compiler::fingerprint:     dirty: UnitDependencyInfoChanged { unit: UnitIndex(171) }
"#;
        let reasons = parse_fingerprint_reasons(log);
        assert_eq!(reasons.len(), 3);
        assert!(
            reasons
                .iter()
                .any(|reason| reason.category == FingerprintCategory::ChangedFile)
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.category == FingerprintCategory::BuildScriptPathsChanged)
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.category == FingerprintCategory::DependencyChanged)
        );
    }
}
