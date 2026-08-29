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

use crate::{cache, cli::DoctorArgs};

#[derive(Debug, Serialize)]
struct DoctorReport {
    source: PathBuf,
    destination: PathBuf,
    source_head: Option<String>,
    destination_head: Option<String>,
    same_revision: bool,
    source_clean: bool,
    destination_clean: bool,
    tracked_mtimes: Option<TrackedMtimeSummary>,
    manifests: Vec<ManifestReport>,
    probes: Vec<ProbeReport>,
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
    source_cache_exists: bool,
    destination_cache_exists: bool,
    build_scripts: Vec<BuildScriptReport>,
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
    let source = match args.source {
        Some(source) => cache::canonical_dir(&source)?,
        None => cache::find_main_worktree(&destination)?.ok_or_else(|| {
            anyhow!("could not find a separate Git worktree on branch main; pass --from explicitly")
        })?,
    };

    let source_head = git_head(&source)?;
    let destination_head = git_head(&destination)?;
    let source_clean = git_clean(&source)?;
    let destination_clean = git_clean(&destination)?;
    let same_revision = source_head.is_some() && source_head == destination_head;
    let tracked_mtimes = if same_revision && source_clean && destination_clean {
        Some(compare_tracked_mtimes(&source, &destination)?)
    } else {
        None
    };

    cache::assert_compatible_toolchains(&source, &destination)?;
    let source_paths = cache::resolve_manifests(&source, &args.manifests)?;
    let destination_paths = cache::resolve_manifests(&destination, &args.manifests)?;
    if source_paths.len() != destination_paths.len() {
        return Err(anyhow!(
            "source and destination resolved different manifest counts"
        ));
    }

    let mut manifests = Vec::with_capacity(destination_paths.len());
    for (source_paths, destination_paths) in source_paths.iter().zip(&destination_paths) {
        let build_scripts = build_scripts(&destination, &destination_paths.manifest)?;
        manifests.push(ManifestReport {
            manifest: destination_paths.manifest.clone(),
            source_build_directory: source_paths.build_directory.clone(),
            destination_build_directory: destination_paths.build_directory.clone(),
            source_cache_exists: source_paths
                .build_directory
                .as_ref()
                .is_some_and(|path| path.is_dir()),
            destination_cache_exists: destination_paths
                .build_directory
                .as_ref()
                .is_some_and(|path| path.is_dir()),
            build_scripts,
        });
    }

    let probes = if args.probe {
        let mut probes = Vec::with_capacity(destination_paths.len());
        for paths in &destination_paths {
            probes.push(probe_manifest(&destination, &paths.manifest)?);
        }
        probes
    } else {
        Vec::new()
    };

    let report = DoctorReport {
        source,
        destination,
        source_head,
        destination_head,
        same_revision,
        source_clean,
        destination_clean,
        tracked_mtimes,
        manifests,
        probes,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, args.probe);
    }
    Ok(())
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

fn probe_manifest(workspace: &Path, manifest: &Path) -> Result<ProbeReport> {
    let started = Instant::now();
    let output = Command::new("cargo")
        .current_dir(workspace)
        .env("CARGO_LOG", "cargo::core::compiler::fingerprint=info")
        .args(["check", "--manifest-path"])
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
    println!("  source:      {}", report.source.display());
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
    } else {
        println!("  mtimes:      skipped (requires clean worktrees at the same revision)");
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
        if manifest.build_scripts.is_empty() {
            println!("  build scripts: none in workspace packages");
        } else {
            println!("  build scripts: {}", manifest.build_scripts.len());
            for script in &manifest.build_scripts {
                println!("    {}: {}", script.package, script.path.display());
            }
            println!(
                "  note: build-script watched files remain mtime-sensitive in Cargo's checksum-freshness experiment; build-script outputs can also contain checkout-local paths"
            );
        }
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
INFO prepare_target{force=false package_id=goldengoose v0.6.4 (/tmp/wt/src-tauri) target="goldengoose_lib"}: cargo::core::compiler::fingerprint: stale: changed "/tmp/wt/src-tauri/build.rs"
INFO prepare_target{force=false package_id=goldengoose v0.6.4 (/tmp/wt/src-tauri) target="build-script-build"}: cargo::core::compiler::fingerprint:     dirty: RerunIfChangedOutputPathsChanged { old: ["build.rs"], new: ["/tmp/wt/build.rs"] }
INFO prepare_target{force=false package_id=goldengoose v0.6.4 (/tmp/wt/src-tauri) target="goldengoose_lib"}: cargo::core::compiler::fingerprint:     dirty: UnitDependencyInfoChanged { unit: UnitIndex(171) }
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
