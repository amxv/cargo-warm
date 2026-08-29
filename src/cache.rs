use std::{
    collections::BTreeMap,
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
    #[serde(skip_serializing)]
    pub(crate) package_roots: BTreeMap<String, PathBuf>,
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

pub(crate) struct LockfileGuard {
    path: PathBuf,
    existed: bool,
}

impl Drop for LockfileGuard {
    fn drop(&mut self) {
        if !self.existed {
            let _ = fs::remove_file(&self.path);
        }
    }
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
    let metadata = cargo_metadata_value(workspace, &manifest)?;
    let workspace_root = json_path(&metadata, "workspace_root")?;
    let target_directory = json_path(&metadata, "target_directory")?;
    let build_directory = metadata
        .get("build_directory")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let mut package_roots = BTreeMap::new();
    if let Some(packages) = metadata.get("packages").and_then(Value::as_array) {
        for package in packages {
            let (Some(name), Some(manifest_path)) = (
                package.get("name").and_then(Value::as_str),
                package.get("manifest_path").and_then(Value::as_str),
            ) else {
                continue;
            };
            if let Some(root) = Path::new(manifest_path).parent() {
                package_roots.insert(name.to_string(), root.to_path_buf());
            }
        }
    }
    Ok(CargoPaths {
        workspace: PathBuf::from(workspace_root),
        manifest,
        build_directory,
        target_directory: PathBuf::from(target_directory),
        package_roots,
    })
}

pub(crate) fn cargo_metadata_value(workspace: &Path, manifest: &Path) -> Result<Value> {
    let _lock_guard = lockfile_guard(workspace, manifest)?;
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
            .arg(manifest),
    )?;
    Ok(serde_json::from_slice(&output)?)
}

pub(crate) fn lockfile_guard(workspace: &Path, manifest: &Path) -> Result<LockfileGuard> {
    let output = command_output(
        Command::new("cargo")
            .current_dir(workspace)
            .args([
                "locate-project",
                "--workspace",
                "--message-format",
                "plain",
                "--manifest-path",
            ])
            .arg(manifest),
    )?;
    let workspace_manifest = PathBuf::from(String::from_utf8(output)?.trim());
    let root = workspace_manifest.parent().ok_or_else(|| {
        anyhow!(
            "workspace manifest has no parent: {}",
            workspace_manifest.display()
        )
    })?;
    let path = root.join("Cargo.lock");
    Ok(LockfileGuard {
        existed: path.exists(),
        path,
    })
}

pub(crate) fn assert_compatible_toolchains(source: &Path, destination: &Path) -> Result<()> {
    if !toolchains_compatible(source, destination)? {
        bail!("Cargo/rustc differ between source and destination; refusing incompatible seed");
    }
    Ok(())
}

pub(crate) fn toolchains_compatible(source: &Path, destination: &Path) -> Result<bool> {
    for tool in ["cargo", "rustc"] {
        let args: &[&str] = if tool == "rustc" { &["-vV"] } else { &["-V"] };
        let source_id = command_output(Command::new(tool).current_dir(source).args(args))?;
        let destination_id =
            command_output(Command::new(tool).current_dir(destination).args(args))?;
        if source_id != destination_id {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn assert_workspace_quiescent(workspace: &Path) -> Result<()> {
    if !workspace_quiescent(workspace)? {
        bail!(
            "Cargo or rustc is active in {}; seed only from a quiescent workspace",
            workspace.display()
        );
    }
    Ok(())
}

pub(crate) fn workspace_quiescent(workspace: &Path) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        workspace_quiescent_macos(workspace)
    }

    #[cfg(not(target_os = "macos"))]
    {
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
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(target_os = "macos")]
fn workspace_quiescent_macos(workspace: &Path) -> Result<bool> {
    let mut pids = Vec::new();
    for process_name in ["cargo", "rustc"] {
        let output = Command::new("pgrep")
            .args(["-x", process_name])
            .output()
            .with_context(|| "failed to inspect running compiler processes")?;
        if output.status.success() {
            pids.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(str::trim)
                    .filter(|pid| !pid.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    if pids.is_empty() {
        return Ok(true);
    }
    pids.sort();
    pids.dedup();
    let output = Command::new("lsof")
        .args(["-a", "-d", "cwd", "-p", &pids.join(","), "-Fn"])
        .output()
        .with_context(|| "failed to inspect compiler working directories")?;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(path) = line.strip_prefix('n')
            && Path::new(path).starts_with(workspace)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn seedable_state_count(paths: &[CargoPaths], include_target: bool) -> usize {
    paths
        .iter()
        .map(|paths| {
            let build = usize::from(
                paths
                    .build_directory
                    .as_ref()
                    .is_some_and(|path| path.is_dir()),
            );
            let target = usize::from(
                (include_target || paths.build_directory.is_none())
                    && paths.target_directory.is_dir(),
            );
            build + target
        })
        .sum()
}

#[cfg(target_os = "linux")]
fn process_cwd(pid: &str) -> Result<Option<PathBuf>> {
    let path = PathBuf::from(format!("/proc/{pid}/cwd"));
    match fs::read_link(path) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
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

pub(crate) fn clone_directory(
    source: &Path,
    destination: &Path,
    strategy: CloneStrategy,
) -> Result<()> {
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

pub(crate) fn seed_workspace_path(
    source_workspace: &Path,
    destination_workspace: &Path,
    relative: &Path,
    strategy: CloneStrategy,
) -> Result<bool> {
    use std::path::Component;
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "--seed-path must be workspace-relative without '..': {}",
            relative.display()
        );
    }
    let source = source_workspace.join(relative);
    let destination = destination_workspace.join(relative);
    let source_metadata = match fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "cargo-warm: extra source does not exist, skipping {}",
                source.display()
            );
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    if destination.exists() {
        println!(
            "cargo-warm: extra destination already exists, leaving untouched: {}",
            destination.display()
        );
        return Ok(false);
    }
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("extra cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".cargo-warm-extra-{}", std::process::id()));
    if temp.exists() {
        if temp.is_dir() {
            fs::remove_dir_all(&temp)?;
        } else {
            fs::remove_file(&temp)?;
        }
    }
    if source_metadata.file_type().is_symlink() {
        let target = fs::read_link(&source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &temp)?;
        #[cfg(windows)]
        {
            if source.is_dir() {
                std::os::windows::fs::symlink_dir(target, &temp)?;
            } else {
                std::os::windows::fs::symlink_file(target, &temp)?;
            }
        }
    } else if source_metadata.is_dir() {
        clone_directory(&source, &temp, strategy)?;
    } else {
        let status = match strategy {
            CloneStrategy::MacClone => Command::new("cp")
                .arg("-c")
                .arg(&source)
                .arg(&temp)
                .status()?,
            CloneStrategy::LinuxReflink => Command::new("cp")
                .args(["--reflink=always", "-p"])
                .arg(&source)
                .arg(&temp)
                .status()?,
            CloneStrategy::PhysicalCopy => Command::new("cp")
                .arg("-p")
                .arg(&source)
                .arg(&temp)
                .status()?,
        };
        if !status.success() {
            bail!("copy command exited with {status}");
        }
    }
    fs::rename(&temp, &destination)?;
    println!("cargo-warm: seeded extra path {}", relative.display());
    Ok(true)
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
