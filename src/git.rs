use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow};

#[derive(Debug, Clone)]
pub(crate) struct Worktree {
    pub(crate) path: PathBuf,
    pub(crate) head: String,
    pub(crate) branch: Option<String>,
    pub(crate) dirty: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceSelection {
    pub(crate) path: PathBuf,
    pub(crate) reason: String,
}

pub(crate) fn best_source_worktree(destination: &Path) -> Result<Option<SourceSelection>> {
    Ok(source_worktree_candidates(destination)?.into_iter().next())
}

pub(crate) fn source_worktree_candidates(destination: &Path) -> Result<Vec<SourceSelection>> {
    let destination = destination.canonicalize()?;
    let destination_head = git_text(&destination, &["rev-parse", "HEAD"])?;
    let mut candidates = worktrees(&destination)?;
    candidates.retain(|candidate| candidate.path != destination && candidate.path.is_dir());

    let mut ranked = Vec::new();
    for candidate in candidates {
        let exact = candidate.head == destination_head;
        let (distance, relation) = if exact {
            (0_u64, "exact revision".to_string())
        } else {
            let Ok(merge_base) = git_text(
                &destination,
                &["merge-base", &candidate.head, &destination_head],
            ) else {
                // The same repository can contain orphan/unrelated histories.
                // They are not useful near-hit seeds for this destination.
                continue;
            };
            let candidate_distance = git_text(
                &destination,
                &[
                    "rev-list",
                    "--count",
                    &format!("{merge_base}..{}", candidate.head),
                ],
            )?
            .parse::<u64>()?;
            let destination_distance = git_text(
                &destination,
                &[
                    "rev-list",
                    "--count",
                    &format!("{merge_base}..{destination_head}"),
                ],
            )?
            .parse::<u64>()?;
            let relation = match (candidate_distance, destination_distance) {
                (0, behind) => format!("ancestor {behind} commit(s) behind"),
                (ahead, 0) => format!("descendant {ahead} commit(s) ahead"),
                (candidate_side, destination_side) => format!(
                    "nearby branch, {candidate_side}+{destination_side} commit(s) from merge base"
                ),
            };
            (candidate_distance + destination_distance, relation)
        };
        let main = candidate.branch.as_deref() == Some("refs/heads/main");
        // Nearness comes first. A quiescent dirty worktree is still a safe
        // incremental starting point because Cargo/rustc validate the forked
        // state and freshness rebasing never blesses dirty source files.
        ranked.push((distance, candidate.dirty, !main, relation, candidate));
    }

    ranked.sort_by_key(|a| (a.0, a.1, a.2));
    Ok(ranked
        .into_iter()
        .map(|(_, _, _, relation, candidate)| {
            let cleanliness = if candidate.dirty { ", dirty" } else { "" };
            SourceSelection {
                path: candidate.path,
                reason: format!("{relation}{cleanliness}"),
            }
        })
        .collect())
}

pub(crate) fn tracked_index_entries(workspace: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(["ls-files", "--stage", "-z"])
        .output()
        .with_context(|| "failed to inspect Git index")?;
    if !output.status.success() {
        return Err(anyhow!("git ls-files failed in {}", workspace.display()));
    }

    let mut entries = BTreeMap::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|r| !r.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let header = String::from_utf8_lossy(&record[..tab]);
        let mut fields = header.split_whitespace();
        let _mode = fields.next();
        let Some(oid) = fields.next() else { continue };
        let stage = fields.next().unwrap_or_default();
        if stage != "0" {
            continue;
        }
        entries.insert(
            PathBuf::from(os_string(&record[tab + 1..])),
            oid.to_string(),
        );
    }
    Ok(entries)
}

pub(crate) fn dirty_tracked_paths(workspace: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut dirty = BTreeSet::new();
    for args in [
        ["diff", "--name-only", "-z"].as_slice(),
        ["diff", "--cached", "--name-only", "-z"].as_slice(),
    ] {
        let output = Command::new("git")
            .current_dir(workspace)
            .args(args)
            .output()
            .with_context(|| "failed to inspect Git changes")?;
        if !output.status.success() {
            return Err(anyhow!("git diff failed in {}", workspace.display()));
        }
        for record in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|r| !r.is_empty())
        {
            dirty.insert(PathBuf::from(os_string(record)));
        }
    }
    Ok(dirty)
}

pub(crate) fn repo_root(workspace: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(git_text(
        workspace,
        &["rev-parse", "--show-toplevel"],
    )?))
}

pub(crate) fn same_repository(a: &Path, b: &Path) -> Result<bool> {
    let a_common = git_common_dir(a)?;
    let b_common = git_common_dir(b)?;
    Ok(a_common == b_common)
}

pub(crate) fn is_ignored(workspace: &Path, relative: &Path) -> Result<bool> {
    let status = Command::new("git")
        .current_dir(workspace)
        .args(["check-ignore", "-q", "--"])
        .arg(relative)
        .status()
        .with_context(|| "failed to inspect Git ignore rules")?;
    Ok(status.success())
}

fn worktrees(workspace: &Path) -> Result<Vec<Worktree>> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .with_context(|| "failed to inspect Git worktrees")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    let mut path = None;
    let mut head = None;
    let mut branch = None;
    let mut flush =
        |path: &mut Option<PathBuf>, head: &mut Option<String>, branch: &mut Option<String>| {
            if let (Some(path), Some(head)) = (path.take(), head.take()) {
                let dirty = is_dirty(&path).unwrap_or(true);
                result.push(Worktree {
                    path,
                    head,
                    branch: branch.take(),
                    dirty,
                });
            }
        };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(value) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut head, &mut branch);
            path = Some(
                PathBuf::from(value)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(value)),
            );
        } else if let Some(value) = line.strip_prefix("HEAD ") {
            head = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("branch ") {
            branch = Some(value.to_string());
        } else if line.is_empty() {
            flush(&mut path, &mut head, &mut branch);
        }
    }
    flush(&mut path, &mut head, &mut branch);
    Ok(result)
}

fn is_dirty(workspace: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(["status", "--porcelain=v1", "--untracked-files=no"])
        .output()?;
    Ok(!output.stdout.is_empty())
}

fn git_common_dir(workspace: &Path) -> Result<PathBuf> {
    let raw = PathBuf::from(git_text(workspace, &["rev-parse", "--git-common-dir"])?);
    let path = if raw.is_absolute() {
        raw
    } else {
        workspace.join(raw)
    };
    Ok(path.canonicalize()?)
}

fn git_text(workspace: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed in {}",
            args.join(" "),
            workspace.display()
        ));
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

#[cfg(unix)]
fn os_string(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

#[cfg(not(unix))]
fn os_string(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use super::best_source_worktree;

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

    #[test]
    fn exact_clean_worktree_is_preferred_as_seed_source() {
        let root =
            std::env::temp_dir().join(format!("cargo-warm-source-select-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let main = root.join("main");
        let feature = root.join("feature");
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-b", "main"]);
        git(
            &main,
            &["config", "user.email", "cargo-warm@example.invalid"],
        );
        git(&main, &["config", "user.name", "cargo-warm"]);
        fs::write(main.join("README"), "seed\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-m", "seed"]);
        git(
            &main,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature.to_str().unwrap(),
            ],
        );

        let selection = best_source_worktree(&feature).unwrap().unwrap();
        assert_eq!(selection.path, main.canonicalize().unwrap());
        assert!(selection.reason.starts_with("exact revision"));

        let _ = Command::new("git")
            .current_dir(&main)
            .args(["worktree", "remove", "--force", feature.to_str().unwrap()])
            .status();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nearby_sibling_worktree_can_beat_a_distant_main() {
        let root =
            std::env::temp_dir().join(format!("cargo-warm-sibling-select-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let main = root.join("main");
        let sibling = root.join("sibling");
        let destination = root.join("destination");
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-b", "main"]);
        git(
            &main,
            &["config", "user.email", "cargo-warm@example.invalid"],
        );
        git(&main, &["config", "user.name", "cargo-warm"]);
        fs::write(main.join("state"), "0\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-m", "base"]);
        for i in 1..=4 {
            fs::write(main.join("state"), format!("{i}\n")).unwrap();
            git(&main, &["commit", "-am", &format!("main {i}")]);
        }
        git(
            &main,
            &[
                "worktree",
                "add",
                "-b",
                "sibling",
                sibling.to_str().unwrap(),
            ],
        );
        git(
            &main,
            &[
                "worktree",
                "add",
                "-b",
                "destination",
                destination.to_str().unwrap(),
            ],
        );
        fs::write(sibling.join("sibling"), "one\n").unwrap();
        git(&sibling, &["add", "."]);
        git(&sibling, &["commit", "-m", "sibling change"]);
        fs::write(destination.join("destination"), "one\n").unwrap();
        git(&destination, &["add", "."]);
        git(&destination, &["commit", "-m", "destination change"]);
        git(&main, &["reset", "--hard", "HEAD~4"]);

        let selection = best_source_worktree(&destination).unwrap().unwrap();
        assert_eq!(selection.path, sibling.canonicalize().unwrap());
        assert!(selection.reason.starts_with("nearby branch"));

        for worktree in [&sibling, &destination] {
            let _ = Command::new("git")
                .current_dir(&main)
                .args(["worktree", "remove", "--force"])
                .arg(worktree)
                .status();
        }
        let _ = fs::remove_dir_all(&root);
    }
}
