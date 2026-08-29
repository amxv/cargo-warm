use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use filetime::{FileTime, set_file_times};
use serde_json::Value;

use crate::cache;

#[derive(Debug)]
struct SavedTimes {
    path: PathBuf,
    atime: FileTime,
    mtime: FileTime,
}

struct TimestampGuard {
    saved: Vec<SavedTimes>,
}

impl TimestampGuard {
    fn touch(paths: Vec<PathBuf>) -> Result<Self> {
        let now = FileTime::from_system_time(SystemTime::now());
        let mut saved = Vec::with_capacity(paths.len());
        for path in paths {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("failed to inspect prime trigger {}", path.display()))?;
            let times = SavedTimes {
                path: path.clone(),
                atime: FileTime::from_last_access_time(&metadata),
                mtime: FileTime::from_last_modification_time(&metadata),
            };
            set_file_times(&path, times.atime, now)
                .with_context(|| format!("failed to touch prime trigger {}", path.display()))?;
            saved.push(times);
        }
        Ok(Self { saved })
    }

    fn restore(&self) -> Result<()> {
        let mut first_error = None;
        for saved in &self.saved {
            if let Err(error) = set_file_times(&saved.path, saved.atime, saved.mtime)
                && first_error.is_none()
            {
                first_error = Some(anyhow!(error).context(format!(
                    "failed to restore prime trigger timestamp {}",
                    saved.path.display()
                )));
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for TimestampGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub(crate) fn run(
    destination: &Path,
    manifests: &[PathBuf],
    unstable_bootstrap: bool,
) -> Result<Duration> {
    let triggers = prime_triggers(destination, manifests)?;
    if triggers.is_empty() {
        bail!("could not find a Rust target source file to trigger relocation priming");
    }

    let guard = TimestampGuard::touch(triggers)?;
    let started = Instant::now();
    let current_exe = env::current_exe().context("failed to resolve cargo-warm executable")?;

    let mut unique_manifests = BTreeSet::new();
    for manifest in manifests {
        let manifest = if manifest.is_absolute() {
            manifest.clone()
        } else {
            destination.join(manifest)
        };
        unique_manifests.insert(manifest);
    }

    let mut check_error = None;
    for manifest in unique_manifests {
        let _lock_guard = cache::lockfile_guard(destination, &manifest)?;
        let mut command = Command::new(&current_exe);
        command.current_dir(destination).arg("check");
        if unstable_bootstrap {
            command.arg("--unstable-bootstrap");
        }
        command.arg("--manifest-path").arg(&manifest);
        let status = command.status().with_context(|| {
            format!(
                "failed to start relocatable prime for {}",
                manifest.display()
            )
        })?;
        if !status.success() {
            check_error = Some(anyhow!(
                "relocatable prime failed for {} with {status}",
                manifest.display()
            ));
            break;
        }
    }

    let restore_result = guard.restore();
    std::mem::forget(guard);
    if let Some(error) = check_error {
        restore_result?;
        return Err(error);
    }
    restore_result?;
    Ok(started.elapsed())
}

fn prime_triggers(workspace: &Path, manifests: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let workspace = workspace.canonicalize()?;
    let mut triggers = BTreeSet::new();
    for manifest in manifests {
        let manifest = if manifest.is_absolute() {
            manifest.clone()
        } else {
            workspace.join(manifest)
        };
        let manifest = manifest
            .canonicalize()
            .with_context(|| format!("prime manifest does not exist: {}", manifest.display()))?;
        let metadata = cache::cargo_metadata_value(&workspace, &manifest)?;
        let packages = metadata
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("cargo metadata did not report packages"))?;

        let mut by_id = BTreeMap::new();
        for package in packages {
            if let Some(id) = package.get("id").and_then(Value::as_str) {
                by_id.insert(id.to_string(), package);
            }
        }

        let direct = packages.iter().find(|package| {
            package
                .get("manifest_path")
                .and_then(Value::as_str)
                .and_then(|path| Path::new(path).canonicalize().ok())
                .is_some_and(|path| path == manifest)
        });
        if let Some(package) = direct {
            for path in package_triggers(package, &workspace)? {
                triggers.insert(path);
            }
            continue;
        }

        let member_ids = metadata
            .get("workspace_default_members")
            .and_then(Value::as_array)
            .filter(|members| !members.is_empty())
            .or_else(|| metadata.get("workspace_members").and_then(Value::as_array));
        if let Some(member_ids) = member_ids {
            for id in member_ids.iter().filter_map(Value::as_str) {
                if let Some(package) = by_id.get(id) {
                    for path in package_triggers(package, &workspace)? {
                        triggers.insert(path);
                    }
                }
            }
        }
    }
    Ok(triggers.into_iter().collect())
}

fn package_triggers(package: &Value, workspace: &Path) -> Result<Vec<PathBuf>> {
    let Some(targets) = package.get("targets").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut triggers = BTreeSet::new();
    let preferred = targets
        .iter()
        .find(|target| target_has_kind(target, "lib"))
        .or_else(|| targets.iter().find(|target| target_has_kind(target, "bin")))
        .or_else(|| {
            targets
                .iter()
                .find(|target| !target_has_kind(target, "custom-build"))
        });
    if let Some(target) = preferred {
        insert_target_source(&mut triggers, target, workspace)?;
    }

    // A package build script is a Cargo fingerprint boundary for the local
    // crate. Re-running that boundary once in the destination makes the
    // inherited rustc state genuinely destination-native for the first real
    // edit. Only the selected package's own custom-build target is touched;
    // path dependencies remain untouched and therefore do not wake unrelated
    // native toolchains.
    for target in targets
        .iter()
        .filter(|target| target_has_kind(target, "custom-build"))
    {
        insert_target_source(&mut triggers, target, workspace)?;
    }

    Ok(triggers.into_iter().collect())
}

fn insert_target_source(
    triggers: &mut BTreeSet<PathBuf>,
    target: &Value,
    workspace: &Path,
) -> Result<()> {
    let Some(src_path) = target.get("src_path").and_then(Value::as_str) else {
        return Ok(());
    };
    let path = PathBuf::from(src_path).canonicalize()?;
    if !path.starts_with(workspace) {
        bail!(
            "refusing relocation-prime trigger outside workspace: {}",
            path.display()
        );
    }
    triggers.insert(path);
    Ok(())
}

fn target_has_kind(target: &Value, expected: &str) -> bool {
    target
        .get("kind")
        .and_then(Value::as_array)
        .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some(expected)))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use filetime::{FileTime, set_file_mtime};

    #[test]
    fn selects_library_source_and_restores_timestamp() {
        let root =
            std::env::temp_dir().join(format!("cargo-warm-prime-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='prime-fixture'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
        fs::write(root.join("build.rs"), "fn main() {}\n").unwrap();

        let triggers = super::prime_triggers(&root, &["Cargo.toml".into()]).unwrap();
        assert_eq!(
            triggers,
            [
                root.join("build.rs").canonicalize().unwrap(),
                root.join("src/lib.rs").canonicalize().unwrap(),
            ]
        );
        assert!(!root.join("Cargo.lock").exists());

        let original = FileTime::from_unix_time(1_600_000_000, 123);
        for trigger in &triggers {
            set_file_mtime(trigger, original).unwrap();
        }
        {
            let guard = super::TimestampGuard::touch(triggers.clone()).unwrap();
            for trigger in &triggers {
                let changed =
                    FileTime::from_last_modification_time(&fs::metadata(trigger).unwrap());
                assert_ne!(changed, original);
            }
            guard.restore().unwrap();
            std::mem::forget(guard);
        }
        for trigger in &triggers {
            let restored = FileTime::from_last_modification_time(&fs::metadata(trigger).unwrap());
            assert_eq!(restored, original);
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_package_prime_does_not_touch_path_dependency_build_script() {
        let root = std::env::temp_dir().join(format!(
            "cargo-warm-prime-path-dep-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("dep/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='prime-root'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n\n[dependencies]\nprime-dep={path='dep'}\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn root() {}\n").unwrap();
        fs::write(root.join("build.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.join("dep/Cargo.toml"),
            "[package]\nname='prime-dep'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n",
        )
        .unwrap();
        fs::write(root.join("dep/src/lib.rs"), "pub fn dep() {}\n").unwrap();
        fs::write(root.join("dep/build.rs"), "fn main() {}\n").unwrap();

        let triggers = super::prime_triggers(&root, &["Cargo.toml".into()]).unwrap();
        assert_eq!(
            triggers,
            [
                root.join("build.rs").canonicalize().unwrap(),
                root.join("src/lib.rs").canonicalize().unwrap(),
            ]
        );
        assert!(!triggers.contains(&root.join("dep/build.rs").canonicalize().unwrap()));
        assert!(!triggers.contains(&root.join("dep/src/lib.rs").canonicalize().unwrap()));
        assert!(!root.join("Cargo.lock").exists());
        let _ = fs::remove_dir_all(root);
    }
}
