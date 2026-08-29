use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CargoPaths {
    pub(crate) workspace: PathBuf,
    pub(crate) manifest: PathBuf,
    pub(crate) build_directory: Option<PathBuf>,
    pub(crate) target_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SeedRecord {
    pub(crate) workspace: PathBuf,
    pub(crate) source_workspace: PathBuf,
    pub(crate) manifest: PathBuf,
    pub(crate) kind: CacheKind,
    pub(crate) path: PathBuf,
    pub(crate) created_unix_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CacheKind {
    Build,
    Target,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Registry {
    pub(crate) schema: u32,
    pub(crate) records: Vec<SeedRecord>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CloneStrategy {
    MacClone,
    LinuxReflink,
    PhysicalCopy,
}

pub(crate) fn find_main_worktree(destination: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .current_dir(destination)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .with_context(|| "failed to inspect Git worktrees")?;
    if !output.status.success() {
        return Ok(None);
    }

    let mut current_worktree: Option<PathBuf> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_worktree = Some(PathBuf::from(path));
            continue;
        }
        if line == "branch refs/heads/main"
            && let Some(worktree) = current_worktree.take()
        {
            let canonical = canonical_dir(&worktree)?;
            if canonical != destination {
                return Ok(Some(canonical));
            }
        }
        if line.is_empty() {
            current_worktree = None;
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn seed_one(
    kind: CacheKind,
    source_path: &Path,
    destination_path: &Path,
    source_workspace: &Path,
    destination_workspace: &Path,
    manifest: &Path,
    strategy: CloneStrategy,
    registry: &mut Registry,
) -> Result<bool> {
    if source_path == destination_path {
        bail!(
            "refusing shared mutable {:?} directory: {}",
            kind,
            source_path.display()
        );
    }
    if !source_path.is_dir() {
        println!(
            "cargo-warm: {:?} source does not exist, skipping {}",
            kind,
            source_path.display()
        );
        return Ok(false);
    }
    if destination_path.exists() {
        println!(
            "cargo-warm: {:?} destination already exists, leaving untouched: {}",
            kind,
            destination_path.display()
        );
        return Ok(false);
    }

    let parent = destination_path
        .parent()
        .ok_or_else(|| anyhow!("cache path has no parent: {}", destination_path.display()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".cargo-warm-{}-{}",
        destination_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("cache"),
        std::process::id()
    ));
    if temp.exists() {
        fs::remove_dir_all(&temp)?;
    }

    let free_before_kb = filesystem_free_kb(parent).ok();
    let started = Instant::now();
    clone_directory(source_path, &temp, strategy).with_context(|| {
        format!(
            "failed to clone {} to {}",
            source_path.display(),
            destination_path.display()
        )
    })?;
    fs::rename(&temp, destination_path)?;
    let elapsed = started.elapsed();
    let free_after_kb = filesystem_free_kb(parent).ok();

    registry
        .records
        .retain(|record| record.path != destination_path);
    registry.records.push(SeedRecord {
        workspace: destination_workspace.to_path_buf(),
        source_workspace: source_workspace.to_path_buf(),
        manifest: manifest.to_path_buf(),
        kind,
        path: destination_path.to_path_buf(),
        created_unix_seconds: now_unix_seconds(),
    });

    let physical_growth = match (free_before_kb, free_after_kb) {
        (Some(before), Some(after)) => {
            let delta_kb = before.saturating_sub(after);
            format!(", filesystem growth ~{:.1} MiB", delta_kb as f64 / 1024.0)
        }
        _ => String::new(),
    };
    println!(
        "cargo-warm: seeded {:?}: {} -> {} ({:.2}s{})",
        kind,
        source_path.display(),
        destination_path.display(),
        elapsed.as_secs_f64(),
        physical_growth
    );
    Ok(true)
}

fn filesystem_free_kb(path: &Path) -> Result<u64> {
    let output = command_output(Command::new("df").args(["-Pk"]).arg(path))?;
    let text = String::from_utf8_lossy(&output);
    let line = text
        .lines()
        .last()
        .ok_or_else(|| anyhow!("df returned no filesystem row"))?;
    let fields: Vec<_> = line.split_whitespace().collect();
    let available = fields
        .get(3)
        .ok_or_else(|| anyhow!("df output did not contain available blocks"))?;
    available
        .parse::<u64>()
        .with_context(|| format!("invalid df available-block count: {available}"))
}

pub(crate) fn resolve_manifests(
    workspace: &Path,
    manifests: &[PathBuf],
) -> Result<Vec<CargoPaths>> {
    manifests
        .iter()
        .map(|manifest| cargo_paths(workspace, manifest))
        .collect()
}

fn cargo_paths(workspace: &Path, manifest: &Path) -> Result<CargoPaths> {
    let manifest = if manifest.is_absolute() {
        manifest.to_path_buf()
    } else {
        workspace.join(manifest)
    };
    if !manifest.is_file() {
        bail!("manifest does not exist: {}", manifest.display());
    }
    let output = command_output(
        Command::new("cargo")
            .current_dir(workspace)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(&manifest),
    )?;
    let metadata: Value = serde_json::from_slice(&output)?;
    let workspace_root = json_path(&metadata, "workspace_root")?;
    let target_directory = json_path(&metadata, "target_directory")?;
    let build_directory = metadata
        .get("build_directory")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    Ok(CargoPaths {
        workspace: PathBuf::from(workspace_root),
        manifest,
        build_directory,
        target_directory: PathBuf::from(target_directory),
    })
}

pub(crate) fn assert_compatible_toolchains(source: &Path, destination: &Path) -> Result<()> {
    for tool in ["cargo", "rustc"] {
        let args: &[&str] = if tool == "rustc" { &["-vV"] } else { &["-V"] };
        let source_id = command_output(Command::new(tool).current_dir(source).args(args))?;
        let destination_id =
            command_output(Command::new(tool).current_dir(destination).args(args))?;
        if source_id != destination_id {
            bail!("{tool} differs between source and destination; refusing incompatible seed");
        }
    }
    Ok(())
}

pub(crate) fn assert_workspace_quiescent(workspace: &Path) -> Result<()> {
    for process_name in ["cargo", "rustc"] {
        let output = Command::new("pgrep")
            .args(["-x", process_name])
            .output()
            .with_context(|| "failed to inspect running compiler processes")?;
        if !output.status.success() {
            continue;
        }

        for pid in String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|pid| !pid.trim().is_empty())
        {
            if let Some(cwd) = process_cwd(pid.trim())?
                && cwd.starts_with(workspace)
            {
                bail!(
                    "{process_name} is active in {}; seed only from a quiescent workspace",
                    workspace.display()
                );
            }
        }
    }
    Ok(())
}

fn process_cwd(pid: &str) -> Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        let path = PathBuf::from(format!("/proc/{pid}/cwd"));
        return match fs::read_link(path) {
            Ok(path) => Ok(Some(path)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        };
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("lsof")
            .args(["-a", "-p", pid, "-d", "cwd", "-Fn"])
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix('n'))
            .map(PathBuf::from))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        Ok(None)
    }
}

pub(crate) fn clone_strategy(copy_fallback: bool) -> Result<CloneStrategy> {
    if cfg!(target_os = "macos") {
        return Ok(CloneStrategy::MacClone);
    }
    if cfg!(target_os = "linux") {
        return Ok(CloneStrategy::LinuxReflink);
    }
    if copy_fallback {
        return Ok(CloneStrategy::PhysicalCopy);
    }
    bail!(
        "copy-on-write cloning is not implemented for this platform; pass --copy-fallback to opt into a physical copy"
    )
}

fn clone_directory(source: &Path, destination: &Path, strategy: CloneStrategy) -> Result<()> {
    let status = match strategy {
        CloneStrategy::MacClone => Command::new("cp")
            .args(["-cR"])
            .arg(source)
            .arg(destination)
            .status()?,
        CloneStrategy::LinuxReflink => Command::new("cp")
            .args(["-a", "--reflink=always"])
            .arg(source)
            .arg(destination)
            .status()?,
        CloneStrategy::PhysicalCopy => Command::new("cp")
            .args(["-a"])
            .arg(source)
            .arg(destination)
            .status()?,
    };
    if !status.success() {
        bail!("copy command exited with {status}");
    }
    Ok(())
}

fn command_output(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("failed to start {command:?}"))?;
    if !output.status.success() {
        bail!("command failed with {}: {command:?}", output.status);
    }
    Ok(output.stdout)
}

pub(crate) fn canonical_dir(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("workspace does not exist: {}", path.display()))
}

fn json_path<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("cargo metadata did not report {key}"))
}

fn registry_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("cargo-warm/state.json"));
    }
    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".cache/cargo-warm/state.json"))
}

pub(crate) fn read_registry() -> Result<Registry> {
    let path = registry_path()?;
    if !path.exists() {
        return Ok(Registry {
            schema: 1,
            records: Vec::new(),
        });
    }
    let bytes = fs::read(&path)?;
    let registry: Registry = serde_json::from_slice(&bytes)?;
    if registry.schema != 1 {
        bail!("unsupported cargo-warm registry schema {}", registry.schema);
    }
    Ok(registry)
}

pub(crate) fn write_registry(registry: &Registry) -> Result<()> {
    let path = registry_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("registry path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".state-{}.json", std::process::id()));
    fs::write(&temp, serde_json::to_vec_pretty(registry)?)?;
    fs::rename(temp, path)?;
    Ok(())
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    #[test]
    fn main_worktree_is_discovered_from_feature_worktree() {
        let root =
            std::env::temp_dir().join(format!("cargo-warm-worktree-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let repo = root.join("repo");
        let feature = root.join("feature");
        fs::create_dir_all(&repo).unwrap();
        Command::new("git")
            .current_dir(&repo)
            .args(["init", "-b", "main"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&repo)
            .args(["config", "user.email", "cargo-warm@example.invalid"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&repo)
            .args(["config", "user.name", "cargo-warm test"])
            .status()
            .unwrap();
        fs::write(repo.join("README"), "seed").unwrap();
        Command::new("git")
            .current_dir(&repo)
            .args(["add", "README"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&repo)
            .args(["commit", "-m", "seed"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&repo)
            .args([
                "worktree",
                "add",
                "-b",
                "feature",
                feature.to_str().unwrap(),
            ])
            .status()
            .unwrap();

        let discovered = super::find_main_worktree(&feature).unwrap().unwrap();
        assert_eq!(discovered, repo.canonicalize().unwrap());

        let _ = Command::new("git")
            .current_dir(&repo)
            .args(["worktree", "remove", "--force", feature.to_str().unwrap()])
            .status();
        let _ = fs::remove_dir_all(&root);
    }
}
