use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use filetime::{FileTime, set_file_mtime, set_symlink_file_times};
use serde::Serialize;

use crate::git;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PathSensitiveOutput {
    pub(crate) output_file: PathBuf,
    pub(crate) command: String,
    pub(crate) rebasable: bool,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct FreshnessReport {
    pub(crate) same_repository: bool,
    pub(crate) eligible_files: usize,
    pub(crate) synced_files: usize,
    pub(crate) dirty_files: usize,
    pub(crate) different_files: usize,
    pub(crate) missing_files: usize,
    pub(crate) rebased_build_script_directives: usize,
    pub(crate) synced_watched_entries: usize,
    pub(crate) path_sensitive_outputs: Vec<PathSensitiveOutput>,
}

impl FreshnessReport {
    pub(crate) fn blocking_outputs(&self) -> usize {
        self.path_sensitive_outputs
            .iter()
            .filter(|output| !output.rebasable)
            .count()
    }

    pub(crate) fn rebasable_outputs(&self) -> usize {
        self.path_sensitive_outputs
            .iter()
            .filter(|output| output.rebasable)
            .count()
    }
}

pub(crate) fn analyze(
    source_workspace: &Path,
    destination_workspace: &Path,
    source_build_dirs: &[PathBuf],
) -> Result<FreshnessReport> {
    let same_repository =
        git::same_repository(source_workspace, destination_workspace).unwrap_or(false);
    let path_sensitive_outputs =
        scan_path_sensitive_outputs(source_workspace, destination_workspace, source_build_dirs)?;
    if !same_repository {
        return Ok(FreshnessReport {
            same_repository,
            path_sensitive_outputs,
            ..FreshnessReport::default()
        });
    }

    let source_root = git::repo_root(source_workspace)?;
    let destination_root = git::repo_root(destination_workspace)?;
    let source_entries = git::tracked_index_entries(&source_root)?;
    let destination_entries = git::tracked_index_entries(&destination_root)?;
    let source_dirty = git::dirty_tracked_paths(&source_root)?;
    let destination_dirty = git::dirty_tracked_paths(&destination_root)?;

    let mut report = FreshnessReport {
        same_repository,
        path_sensitive_outputs,
        ..FreshnessReport::default()
    };
    count_entries(
        &source_root,
        &destination_root,
        &source_entries,
        &destination_entries,
        &source_dirty,
        &destination_dirty,
        &mut report,
        false,
    )?;
    Ok(report)
}

pub(crate) fn synchronize(
    source_workspace: &Path,
    destination_workspace: &Path,
    build_pairs: &[(PathBuf, PathBuf)],
    package_roots: &BTreeMap<String, PathBuf>,
) -> Result<FreshnessReport> {
    let source_build_dirs: Vec<_> = build_pairs
        .iter()
        .map(|(source, _)| source.clone())
        .collect();
    let mut report = analyze(source_workspace, destination_workspace, &source_build_dirs)?;
    if !report.same_repository {
        return Ok(report);
    }

    report.rebased_build_script_directives = rebase_build_script_outputs(
        source_workspace,
        destination_workspace,
        build_pairs,
        &report.path_sensitive_outputs,
    )?;
    if report.blocking_outputs() > 0 {
        // A cached build-script directive would still point at the source
        // checkout (or at a destination path that does not exist). Do not make
        // Cargo source files look fresh in that situation: forcing the build
        // script to rerun is safer than allowing cross-worktree state leakage.
        return Ok(report);
    }

    report.synced_watched_entries = synchronize_build_script_watched_paths(
        source_workspace,
        destination_workspace,
        build_pairs,
        package_roots,
    )?;

    let source_root = git::repo_root(source_workspace)?;
    let destination_root = git::repo_root(destination_workspace)?;
    let source_entries = git::tracked_index_entries(&source_root)?;
    let destination_entries = git::tracked_index_entries(&destination_root)?;
    let source_dirty = git::dirty_tracked_paths(&source_root)?;
    let destination_dirty = git::dirty_tracked_paths(&destination_root)?;
    report.synced_files = 0;
    count_entries(
        &source_root,
        &destination_root,
        &source_entries,
        &destination_entries,
        &source_dirty,
        &destination_dirty,
        &mut report,
        true,
    )?;
    Ok(report)
}

pub(crate) fn materializable_link_search_paths(
    source_workspace: &Path,
    destination_workspace: &Path,
    source_build_dirs: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let source_root = git::repo_root(source_workspace)?.canonicalize()?;
    let destination_root = git::repo_root(destination_workspace)?.canonicalize()?;
    let source_prefix = source_root.to_string_lossy();
    let mut paths = BTreeSet::new();

    for build_dir in source_build_dirs {
        visit_outputs(build_dir, 0, &mut |output| {
            let Ok(text) = fs::read_to_string(output) else {
                return;
            };
            let link_libs: Vec<_> = text.lines().filter_map(link_lib_name).collect();
            if link_libs.is_empty() {
                return;
            }
            for line in text.lines().map(str::trim) {
                if !(line.starts_with("cargo:rustc-link-search=")
                    || line.starts_with("cargo::rustc-link-search="))
                    || !line.contains(source_prefix.as_ref())
                {
                    continue;
                }
                let Some(start) = line.find(source_prefix.as_ref()) else {
                    continue;
                };
                let source_search = PathBuf::from(&line[start..]);
                let Ok(search_relative) = source_search.strip_prefix(&source_root) else {
                    continue;
                };
                if search_relative.as_os_str().is_empty()
                    || destination_root.join(search_relative).exists()
                    || !git::is_ignored(&source_root, search_relative).unwrap_or(false)
                {
                    continue;
                }
                let Ok(resolved_search) = source_search.canonicalize() else {
                    continue;
                };
                if !resolved_search.starts_with(&source_root) || !resolved_search.is_dir() {
                    continue;
                }

                let Ok(artifacts) = linkable_artifacts(&resolved_search, &link_libs) else {
                    continue;
                };
                if artifacts.is_empty() {
                    continue;
                }
                for artifact in artifacts {
                    let Ok(relative) = artifact.strip_prefix(&source_root) else {
                        continue;
                    };
                    if git::is_ignored(&source_root, relative).unwrap_or(false) {
                        paths.insert(relative.to_path_buf());
                    }
                }

                // If Cargo was given a symlinked search directory, reproduce
                // the symlink after seeding only the final linkable artifacts
                // behind it. This avoids copying opaque native compiler caches
                // (Swift/Clang modules, PCHs, etc.) that may embed checkout
                // paths while still making the cached build-script output safe.
                if fs::symlink_metadata(&source_search)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    paths.insert(search_relative.to_path_buf());
                }
            }
        })?;
    }

    Ok(paths.into_iter().collect())
}

fn link_lib_name(line: &str) -> Option<String> {
    let value = line
        .strip_prefix("cargo:rustc-link-lib=")
        .or_else(|| line.strip_prefix("cargo::rustc-link-lib="))?
        .trim();
    let name = value.rsplit_once('=').map_or(value, |(_, name)| name);
    let name = name.split(':').next().unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn linkable_artifacts(directory: &Path, names: &[String]) -> Result<Vec<PathBuf>> {
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if names
            .iter()
            .any(|name| link_artifact_name_matches(&file_name, name))
        {
            artifacts.push(entry.path());
        }
    }
    artifacts.sort();
    Ok(artifacts)
}

fn link_artifact_name_matches(file_name: &str, library: &str) -> bool {
    let unix_prefix = format!("lib{library}");
    file_name == format!("{unix_prefix}.a")
        || file_name == format!("{unix_prefix}.dylib")
        || file_name == format!("{unix_prefix}.so")
        || file_name.starts_with(&format!("{unix_prefix}.so."))
        || file_name == format!("{library}.lib")
        || file_name == format!("{library}.dll")
        || file_name == format!("{library}.framework")
}

#[allow(clippy::too_many_arguments)]
fn count_entries(
    source_root: &Path,
    destination_root: &Path,
    source_entries: &BTreeMap<PathBuf, String>,
    destination_entries: &BTreeMap<PathBuf, String>,
    source_dirty: &BTreeSet<PathBuf>,
    destination_dirty: &BTreeSet<PathBuf>,
    report: &mut FreshnessReport,
    apply: bool,
) -> Result<()> {
    if !apply {
        report.eligible_files = 0;
        report.dirty_files = 0;
        report.different_files = 0;
        report.missing_files = 0;
    }

    for (relative, source_oid) in source_entries {
        let Some(destination_oid) = destination_entries.get(relative) else {
            if !apply {
                report.missing_files += 1;
            }
            continue;
        };
        if source_dirty.contains(relative) || destination_dirty.contains(relative) {
            if !apply {
                report.dirty_files += 1;
            }
            continue;
        }
        if source_oid != destination_oid {
            if !apply {
                report.different_files += 1;
            }
            continue;
        }
        let source = source_root.join(relative);
        let destination = destination_root.join(relative);
        let (Ok(source_meta), Ok(destination_meta)) = (
            fs::symlink_metadata(&source),
            fs::symlink_metadata(&destination),
        ) else {
            if !apply {
                report.missing_files += 1;
            }
            continue;
        };
        if !source_meta.file_type().is_file() || !destination_meta.file_type().is_file() {
            continue;
        }
        // Git index identity is a strong filter, but compare the bytes before
        // changing filesystem freshness metadata. This keeps the optimization
        // correct even under racy-stat edge cases or unusual Git settings.
        if !files_equal(&source, &destination)? {
            if !apply {
                report.different_files += 1;
            }
            continue;
        }
        if !apply {
            report.eligible_files += 1;
            continue;
        }
        let mtime = FileTime::from_last_modification_time(&source_meta);
        set_file_mtime(&destination, mtime)
            .with_context(|| format!("failed to mirror mtime for {}", destination.display()))?;
        report.synced_files += 1;
    }
    Ok(())
}

fn scan_path_sensitive_outputs(
    source_workspace: &Path,
    destination_workspace: &Path,
    build_dirs: &[PathBuf],
) -> Result<Vec<PathSensitiveOutput>> {
    let source = source_workspace
        .canonicalize()?
        .to_string_lossy()
        .into_owned();
    let destination = destination_workspace
        .canonicalize()?
        .to_string_lossy()
        .into_owned();
    let mut found = Vec::new();
    for build_dir in build_dirs {
        visit_outputs(build_dir, 0, &mut |output| {
            let Ok(metadata) = fs::metadata(output) else {
                return;
            };
            if metadata.len() > 4 * 1024 * 1024 {
                return;
            }
            let Ok(text) = fs::read_to_string(output) else {
                return;
            };
            for line in text.lines() {
                let trimmed = line.trim();
                if !is_path_sensitive_directive(trimmed, &source) {
                    continue;
                }
                let (rebasable, reason) = classify_path_directive(trimmed, &source, &destination);
                found.push(PathSensitiveOutput {
                    output_file: output.to_path_buf(),
                    command: trimmed.to_string(),
                    rebasable,
                    reason,
                });
            }
        })?;
    }
    found.sort_by(|a, b| (&a.output_file, &a.command).cmp(&(&b.output_file, &b.command)));
    found.dedup_by(|a, b| a.output_file == b.output_file && a.command == b.command);
    Ok(found)
}

fn is_path_sensitive_directive(line: &str, source: &str) -> bool {
    (line.starts_with("cargo:") || line.starts_with("cargo::"))
        && line.contains(source)
        && !line.starts_with("cargo:warning=")
        && !line.starts_with("cargo::warning=")
}

fn classify_path_directive(line: &str, source: &str, destination: &str) -> (bool, Option<String>) {
    let safe = [
        "cargo:rerun-if-changed=",
        "cargo::rerun-if-changed=",
        "cargo:rustc-link-search=",
        "cargo::rustc-link-search=",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix));
    if !safe {
        return (
            false,
            Some("directive embeds the source checkout in an unsupported field".to_string()),
        );
    }

    let Some(index) = line.find(source) else {
        return (false, Some("source path could not be isolated".to_string()));
    };
    let suffix = &line[index + source.len()..];
    let destination_path = PathBuf::from(format!("{destination}{suffix}"));
    if destination_path.exists() {
        (true, None)
    } else {
        (
            false,
            Some(format!(
                "equivalent destination path does not exist: {}",
                destination_path.display()
            )),
        )
    }
}

fn rebase_build_script_outputs(
    source_workspace: &Path,
    destination_workspace: &Path,
    build_pairs: &[(PathBuf, PathBuf)],
    outputs: &[PathSensitiveOutput],
) -> Result<usize> {
    if outputs.is_empty() {
        return Ok(0);
    }
    let source = source_workspace
        .canonicalize()?
        .to_string_lossy()
        .into_owned();
    let destination = destination_workspace
        .canonicalize()?
        .to_string_lossy()
        .into_owned();
    let rebasable: BTreeSet<_> = outputs
        .iter()
        .filter(|output| output.rebasable)
        .map(|output| (output.output_file.clone(), output.command.clone()))
        .collect();
    if rebasable.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for (source_build, destination_build) in build_pairs {
        visit_outputs(source_build, 0, &mut |source_output| {
            let Ok(relative) = source_output.strip_prefix(source_build) else {
                return;
            };
            let destination_output = destination_build.join(relative);
            if !destination_output.is_file() {
                return;
            }
            let Ok(text) = fs::read_to_string(&destination_output) else {
                return;
            };
            let mut changed = false;
            let mut rewritten = String::with_capacity(text.len());
            for chunk in text.split_inclusive('\n') {
                let line = chunk.strip_suffix('\n').unwrap_or(chunk);
                let newline = if chunk.ends_with('\n') { "\n" } else { "" };
                if rebasable.contains(&(source_output.to_path_buf(), line.trim().to_string())) {
                    rewritten.push_str(&line.replace(&source, &destination));
                    changed = true;
                    count += 1;
                } else {
                    rewritten.push_str(line);
                }
                rewritten.push_str(newline);
            }
            if changed
                && fs::write(&destination_output, rewritten).is_ok()
                && let Ok(metadata) = fs::metadata(source_output)
            {
                let mtime = FileTime::from_last_modification_time(&metadata);
                let _ = set_file_mtime(&destination_output, mtime);
            }
        })?;
    }
    Ok(count)
}

fn synchronize_build_script_watched_paths(
    source_workspace: &Path,
    destination_workspace: &Path,
    build_pairs: &[(PathBuf, PathBuf)],
    package_roots: &BTreeMap<String, PathBuf>,
) -> Result<usize> {
    let source_root = git::repo_root(source_workspace)?.canonicalize()?;
    let destination_root = git::repo_root(destination_workspace)?.canonicalize()?;
    let mut watched = BTreeSet::new();

    for (source_build, _) in build_pairs {
        visit_fingerprint_json(source_build, 0, &mut |fingerprint| {
            let Some(package_name) = fingerprint_package_name(fingerprint) else {
                return;
            };
            let Some(package_root) = package_roots.get(&package_name) else {
                return;
            };
            let Ok(bytes) = fs::read(fingerprint) else {
                return;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                return;
            };
            let Some(local) = value.get("local").and_then(serde_json::Value::as_array) else {
                return;
            };
            for item in local {
                let Some(paths) = item
                    .get("RerunIfChanged")
                    .and_then(|entry| entry.get("paths"))
                    .and_then(serde_json::Value::as_array)
                else {
                    continue;
                };
                for path in paths.iter().filter_map(serde_json::Value::as_str) {
                    let raw = PathBuf::from(path);
                    let source_path = if raw.is_absolute() {
                        raw
                    } else {
                        package_root.join(raw)
                    };
                    let Ok(relative) = source_path.strip_prefix(&source_root) else {
                        continue;
                    };
                    watched.insert(relative.to_path_buf());
                }
            }
        })?;
    }

    let mut synced = 0;
    for relative in watched {
        let source = source_root.join(&relative);
        let destination = destination_root.join(&relative);
        if !source.exists() || !destination.exists() {
            continue;
        }
        if trees_equivalent(&source, &destination)? {
            synced += mirror_tree_mtimes(&source, &destination)?;
        }
    }
    Ok(synced)
}

fn visit_fingerprint_json(
    path: &Path,
    depth: usize,
    visitor: &mut impl FnMut(&Path),
) -> Result<()> {
    if depth > 5 || !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            visit_fingerprint_json(&child, depth + 1, visitor)?;
        } else if file_type.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("run-build-script")
            && child.extension().and_then(|ext| ext.to_str()) == Some("json")
        {
            visitor(&child);
        }
    }
    Ok(())
}

fn fingerprint_package_name(path: &Path) -> Option<String> {
    let directory = path.parent()?.file_name()?.to_str()?;
    let (name, hash) = directory.rsplit_once('-')?;
    if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(name.to_string())
    } else {
        None
    }
}

fn trees_equivalent(source: &Path, destination: &Path) -> Result<bool> {
    let source_meta = fs::symlink_metadata(source)?;
    let destination_meta = fs::symlink_metadata(destination)?;
    if source_meta.file_type().is_symlink() || destination_meta.file_type().is_symlink() {
        return Ok(source_meta.file_type().is_symlink()
            && destination_meta.file_type().is_symlink()
            && fs::read_link(source)? == fs::read_link(destination)?);
    }
    if source_meta.is_file() || destination_meta.is_file() {
        return Ok(source_meta.is_file()
            && destination_meta.is_file()
            && files_equal(source, destination)?);
    }
    if !source_meta.is_dir() || !destination_meta.is_dir() {
        return Ok(false);
    }

    let source_entries = directory_entries(source)?;
    let destination_entries = directory_entries(destination)?;
    if source_entries.keys().ne(destination_entries.keys()) {
        return Ok(false);
    }
    for (name, source_child) in source_entries {
        let destination_child = &destination_entries[&name];
        if !trees_equivalent(&source_child, destination_child)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn directory_entries(path: &Path) -> Result<BTreeMap<std::ffi::OsString, PathBuf>> {
    let mut entries = BTreeMap::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        entries.insert(entry.file_name(), entry.path());
    }
    Ok(entries)
}

fn files_equal(source: &Path, destination: &Path) -> Result<bool> {
    let source_meta = fs::metadata(source)?;
    let destination_meta = fs::metadata(destination)?;
    if source_meta.len() != destination_meta.len() {
        return Ok(false);
    }
    let mut source = BufReader::new(fs::File::open(source)?);
    let mut destination = BufReader::new(fs::File::open(destination)?);
    let mut source_buffer = [0_u8; 64 * 1024];
    let mut destination_buffer = [0_u8; 64 * 1024];
    loop {
        let source_read = source.read(&mut source_buffer)?;
        let destination_read = destination.read(&mut destination_buffer)?;
        if source_read != destination_read
            || source_buffer[..source_read] != destination_buffer[..destination_read]
        {
            return Ok(false);
        }
        if source_read == 0 {
            return Ok(true);
        }
    }
}

fn mirror_tree_mtimes(source: &Path, destination: &Path) -> Result<usize> {
    let source_meta = fs::symlink_metadata(source)?;
    if source_meta.file_type().is_symlink() {
        let atime = FileTime::from_last_access_time(&source_meta);
        let mtime = FileTime::from_last_modification_time(&source_meta);
        set_symlink_file_times(destination, atime, mtime)?;
        return Ok(1);
    }
    if source_meta.is_file() {
        set_file_mtime(
            destination,
            FileTime::from_last_modification_time(&source_meta),
        )?;
        return Ok(1);
    }
    if !source_meta.is_dir() {
        return Ok(0);
    }

    let mut count = 1;
    for (name, source_child) in directory_entries(source)? {
        count += mirror_tree_mtimes(&source_child, &destination.join(name))?;
    }
    set_file_mtime(
        destination,
        FileTime::from_last_modification_time(&source_meta),
    )?;
    Ok(count)
}

fn visit_outputs(path: &Path, depth: usize, visitor: &mut impl FnMut(&Path)) -> Result<()> {
    if depth > 5 || !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            visit_outputs(&child, depth + 1, visitor)?;
        } else if file_type.is_file() && entry.file_name() == "output" {
            visitor(&child);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        process::Command,
        time::{Duration, SystemTime},
    };

    use filetime::{FileTime, set_file_mtime};

    use super::{analyze, materializable_link_search_paths, synchronize};

    fn git(workspace: &std::path::Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .current_dir(workspace)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    fn init_pair(name: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("cargo-warm-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-b", "main"]);
        git(
            &source,
            &["config", "user.email", "cargo-warm@example.invalid"],
        );
        git(&source, &["config", "user.name", "cargo-warm"]);
        fs::write(source.join("file.rs"), "fn main() {}\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-m", "seed"]);
        git(
            &source,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                destination.to_str().unwrap(),
            ],
        );
        (root, source, destination)
    }

    fn cleanup(root: &std::path::Path, source: &std::path::Path, destination: &std::path::Path) {
        let _ = Command::new("git")
            .current_dir(source)
            .args([
                "worktree",
                "remove",
                "--force",
                destination.to_str().unwrap(),
            ])
            .status();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mirrors_mtime_only_for_identical_clean_tracked_files() {
        let (root, source, destination) = init_pair("freshness");
        let old = FileTime::from_system_time(SystemTime::now() - Duration::from_secs(120));
        let new = FileTime::from_system_time(SystemTime::now());
        set_file_mtime(source.join("file.rs"), old).unwrap();
        set_file_mtime(destination.join("file.rs"), new).unwrap();
        let report = synchronize(&source, &destination, &[], &BTreeMap::new()).unwrap();
        assert_eq!(report.synced_files, 1);
        assert_eq!(
            fs::metadata(source.join("file.rs"))
                .unwrap()
                .modified()
                .unwrap(),
            fs::metadata(destination.join("file.rs"))
                .unwrap()
                .modified()
                .unwrap()
        );

        fs::write(
            destination.join("file.rs"),
            "fn main() { println!(\"changed\"); }\n",
        )
        .unwrap();
        let report = synchronize(&source, &destination, &[], &BTreeMap::new()).unwrap();
        assert_eq!(report.synced_files, 0);
        assert!(report.dirty_files >= 1);
        cleanup(&root, &source, &destination);
    }

    #[test]
    fn verifies_bytes_before_mirroring_even_when_git_hides_a_change() {
        let (root, source, destination) = init_pair("byte-proof");
        let old = FileTime::from_system_time(SystemTime::now() - Duration::from_secs(120));
        let new = FileTime::from_system_time(SystemTime::now());
        set_file_mtime(source.join("file.rs"), old).unwrap();
        set_file_mtime(destination.join("file.rs"), new).unwrap();
        git(
            &destination,
            &["update-index", "--assume-unchanged", "file.rs"],
        );
        fs::write(
            destination.join("file.rs"),
            "fn main() { /* hidden change */ }\n",
        )
        .unwrap();

        let report = synchronize(&source, &destination, &[], &BTreeMap::new()).unwrap();
        assert_eq!(report.synced_files, 0);
        assert!(report.different_files >= 1);
        assert_ne!(
            fs::metadata(source.join("file.rs"))
                .unwrap()
                .modified()
                .unwrap(),
            fs::metadata(destination.join("file.rs"))
                .unwrap()
                .modified()
                .unwrap()
        );
        git(
            &destination,
            &["update-index", "--no-assume-unchanged", "file.rs"],
        );
        cleanup(&root, &source, &destination);
    }

    #[test]
    fn missing_destination_link_path_blocks_rebase() {
        let (root, source, destination) = init_pair("path-sensitive");
        let source_build = root.join("source-build/debug/build/pkg-deadbeef");
        let destination_build = root.join("destination-build/debug/build/pkg-deadbeef");
        fs::create_dir_all(&source_build).unwrap();
        fs::create_dir_all(&destination_build).unwrap();
        let source_canonical = source.canonicalize().unwrap();
        let output = format!(
            "cargo:rustc-link-search={}/native\n",
            source_canonical.display()
        );
        fs::write(source_build.join("output"), &output).unwrap();
        fs::write(destination_build.join("output"), &output).unwrap();

        let report = analyze(&source, &destination, &[root.join("source-build")]).unwrap();
        assert_eq!(report.path_sensitive_outputs.len(), 1);
        assert_eq!(report.blocking_outputs(), 1);
        let report = synchronize(
            &source,
            &destination,
            &[(root.join("source-build"), root.join("destination-build"))],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(report.synced_files, 0);
        cleanup(&root, &source, &destination);
    }

    #[test]
    fn ignored_missing_link_search_state_is_auto_materializable() {
        let (root, source, destination) = init_pair("auto-native");
        fs::write(source.join(".gitignore"), "native/\n").unwrap();
        git(&source, &["add", ".gitignore"]);
        git(&source, &["commit", "-m", "ignore native"]);
        git(&destination, &["reset", "--hard", "HEAD"]);
        // The linked worktree is still at the earlier commit. Bring it to the
        // source commit without materializing the ignored generated directory.
        git(&destination, &["reset", "--hard", "main"]);
        fs::create_dir_all(source.join("native")).unwrap();
        fs::write(source.join("native/libexample.a"), "native-cache").unwrap();

        let source_build = root.join("source-build/debug/build/pkg-deadbeef");
        fs::create_dir_all(&source_build).unwrap();
        let source_canonical = source.canonicalize().unwrap();
        fs::write(
            source_build.join("output"),
            format!(
                "cargo:rustc-link-lib=static=example\ncargo:rustc-link-search={}/native\n",
                source_canonical.display()
            ),
        )
        .unwrap();

        let paths =
            materializable_link_search_paths(&source, &destination, &[root.join("source-build")])
                .unwrap();
        assert_eq!(paths, [std::path::PathBuf::from("native/libexample.a")]);
        cleanup(&root, &source, &destination);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_link_search_seeds_only_final_library_and_symlink() {
        use std::os::unix::fs::symlink;

        let (root, source, destination) = init_pair("auto-native-symlink");
        fs::write(source.join(".gitignore"), ".build/\n").unwrap();
        git(&source, &["add", ".gitignore"]);
        git(&source, &["commit", "-m", "ignore native"]);
        git(&destination, &["reset", "--hard", "main"]);

        let native = source.join(".build/arch/debug");
        fs::create_dir_all(&native).unwrap();
        fs::write(native.join("libexample.a"), "final-library").unwrap();
        fs::write(native.join("compiler-module.cache"), "path-sensitive-cache").unwrap();
        symlink("arch/debug", source.join(".build/debug")).unwrap();

        let source_build = root.join("source-build/debug/build/pkg-deadbeef");
        fs::create_dir_all(&source_build).unwrap();
        let source_canonical = source.canonicalize().unwrap();
        fs::write(
            source_build.join("output"),
            format!(
                "cargo:rustc-link-lib=static=example\ncargo:rustc-link-search={}/.build/debug\n",
                source_canonical.display()
            ),
        )
        .unwrap();

        let paths =
            materializable_link_search_paths(&source, &destination, &[root.join("source-build")])
                .unwrap();
        assert_eq!(
            paths,
            [
                std::path::PathBuf::from(".build/arch/debug/libexample.a"),
                std::path::PathBuf::from(".build/debug"),
            ]
        );
        assert!(
            !paths
                .iter()
                .any(|path| path.ends_with("compiler-module.cache"))
        );
        cleanup(&root, &source, &destination);
    }

    #[test]
    fn rebases_safe_build_script_paths_before_mtime_sync() {
        let (root, source, destination) = init_pair("output-rebase");
        fs::create_dir_all(source.join("native")).unwrap();
        fs::create_dir_all(destination.join("native")).unwrap();
        let source_build = root.join("source-build/debug/build/pkg-deadbeef");
        let destination_build = root.join("destination-build/debug/build/pkg-deadbeef");
        fs::create_dir_all(&source_build).unwrap();
        fs::create_dir_all(&destination_build).unwrap();
        let source_canonical = source.canonicalize().unwrap();
        let destination_canonical = destination.canonicalize().unwrap();
        let output = format!(
            "cargo:rerun-if-changed={}/file.rs\ncargo:rustc-link-search={}/native\n",
            source_canonical.display(),
            source_canonical.display()
        );
        fs::write(source_build.join("output"), &output).unwrap();
        fs::write(destination_build.join("output"), &output).unwrap();

        let report = synchronize(
            &source,
            &destination,
            &[(root.join("source-build"), root.join("destination-build"))],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(report.blocking_outputs(), 0);
        assert_eq!(report.rebased_build_script_directives, 2);
        assert_eq!(report.synced_files, 1);
        let rewritten = fs::read_to_string(destination_build.join("output")).unwrap();
        assert!(rewritten.contains(&destination_canonical.to_string_lossy().to_string()));
        assert!(!rewritten.contains(&source_canonical.to_string_lossy().to_string()));
        cleanup(&root, &source, &destination);
    }
}
