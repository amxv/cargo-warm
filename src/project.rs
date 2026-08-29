use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::Value;

use crate::{cache, compiler};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectShape {
    pub(crate) rustc: compiler::RustcInfo,
    pub(crate) manifests: Vec<ManifestShape>,
    pub(crate) rust_source_files: usize,
    pub(crate) rust_lines: usize,
    pub(crate) direct_build_scripts: usize,
    pub(crate) other_local_build_scripts: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ManifestShape {
    pub(crate) manifest: PathBuf,
    pub(crate) workspace_packages: usize,
    pub(crate) selected_packages: Vec<PackageShape>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PackageShape {
    pub(crate) name: String,
    pub(crate) manifest: PathBuf,
    pub(crate) rust_source_files: usize,
    pub(crate) rust_lines: usize,
    pub(crate) has_build_script: bool,
}

pub(crate) fn inspect(workspace: &Path, manifests: &[PathBuf]) -> Result<ProjectShape> {
    let rustc = compiler::rustc_info(workspace)?;
    let mut manifest_shapes = Vec::new();
    let mut totals = BTreeMap::<PathBuf, PackageShape>::new();
    let mut all_local_build_scripts = BTreeSet::new();

    for manifest in manifests {
        let manifest = absolute_manifest(workspace, manifest)?;
        let metadata = cache::cargo_metadata_value(workspace, &manifest)?;
        let packages = metadata
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("cargo metadata did not report packages"))?;

        for package in packages {
            if package_has_build_script(package)
                && let Some(path) = package_manifest(package)
            {
                all_local_build_scripts.insert(path);
            }
        }

        let selected = selected_packages(packages, &metadata, &manifest);
        let mut selected_shapes = Vec::new();
        for package in selected {
            let shape = inspect_package(package)?;
            totals
                .entry(shape.manifest.clone())
                .or_insert_with(|| shape.clone());
            selected_shapes.push(shape);
        }

        manifest_shapes.push(ManifestShape {
            manifest,
            workspace_packages: packages.len(),
            selected_packages: selected_shapes,
        });
    }

    let rust_source_files = totals
        .values()
        .map(|package| package.rust_source_files)
        .sum();
    let rust_lines = totals.values().map(|package| package.rust_lines).sum();
    let direct_build_scripts = totals
        .values()
        .filter(|package| package.has_build_script)
        .count();
    let selected_manifests: BTreeSet<_> = totals.keys().cloned().collect();
    let other_local_build_scripts = all_local_build_scripts
        .difference(&selected_manifests)
        .count();

    Ok(ProjectShape {
        rustc,
        manifests: manifest_shapes,
        rust_source_files,
        rust_lines,
        direct_build_scripts,
        other_local_build_scripts,
    })
}

fn absolute_manifest(workspace: &Path, manifest: &Path) -> Result<PathBuf> {
    let path = if manifest.is_absolute() {
        manifest.to_path_buf()
    } else {
        workspace.join(manifest)
    };
    path.canonicalize()
        .with_context(|| format!("manifest does not exist: {}", path.display()))
}

fn selected_packages<'a>(
    packages: &'a [Value],
    metadata: &Value,
    manifest: &Path,
) -> Vec<&'a Value> {
    if let Some(package) = packages.iter().find(|package| {
        package_manifest(package)
            .and_then(|path| path.canonicalize().ok())
            .is_some_and(|path| path == manifest)
    }) {
        return vec![package];
    }

    let by_id: BTreeMap<_, _> = packages
        .iter()
        .filter_map(|package| Some((package.get("id")?.as_str()?.to_string(), package)))
        .collect();
    metadata
        .get("workspace_default_members")
        .and_then(Value::as_array)
        .filter(|members| !members.is_empty())
        .or_else(|| metadata.get("workspace_members").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|id| by_id.get(id).copied())
        .collect()
}

fn inspect_package(package: &Value) -> Result<PackageShape> {
    let name = package
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>")
        .to_string();
    let manifest = package_manifest(package)
        .ok_or_else(|| anyhow!("package {name} did not report manifest_path"))?;
    let root = manifest
        .parent()
        .ok_or_else(|| anyhow!("package manifest has no parent: {}", manifest.display()))?;
    let src = root.join("src");
    let (rust_source_files, rust_lines) = if src.is_dir() {
        count_rust_tree(&src)?
    } else {
        count_target_roots(package)?
    };
    Ok(PackageShape {
        name,
        manifest,
        rust_source_files,
        rust_lines,
        has_build_script: package_has_build_script(package),
    })
}

fn package_manifest(package: &Value) -> Option<PathBuf> {
    package
        .get("manifest_path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
}

fn package_has_build_script(package: &Value) -> bool {
    package
        .get("targets")
        .and_then(Value::as_array)
        .is_some_and(|targets| {
            targets.iter().any(|target| {
                target
                    .get("kind")
                    .and_then(Value::as_array)
                    .is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|kind| kind.as_str() == Some("custom-build"))
                    })
            })
        })
}

fn count_rust_tree(root: &Path) -> Result<(usize, usize)> {
    let mut files = 0usize;
    let mut lines = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                files += 1;
                lines += count_lines(&path)?;
            }
        }
    }
    Ok((files, lines))
}

fn count_target_roots(package: &Value) -> Result<(usize, usize)> {
    let mut roots = BTreeSet::new();
    if let Some(targets) = package.get("targets").and_then(Value::as_array) {
        for target in targets {
            if target
                .get("kind")
                .and_then(Value::as_array)
                .is_some_and(|kinds| {
                    kinds
                        .iter()
                        .any(|kind| kind.as_str() == Some("custom-build"))
                })
            {
                continue;
            }
            if let Some(path) = target.get("src_path").and_then(Value::as_str) {
                roots.insert(PathBuf::from(path));
            }
        }
    }
    let mut lines = 0usize;
    for path in &roots {
        lines += count_lines(path)?;
    }
    Ok((roots.len(), lines))
}

fn count_lines(path: &Path) -> Result<usize> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to read Rust source {}", path.display()))?;
    Ok(BufReader::new(file).lines().count())
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn counts_only_direct_package_source_tree() {
        let root =
            std::env::temp_dir().join(format!("cargo-warm-project-shape-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join("dep/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='shape-root'\nversion='0.1.0'\nedition='2024'\n\n[dependencies]\nshape-dep={path='dep'}\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub mod nested;\n").unwrap();
        fs::write(root.join("src/nested/mod.rs"), "pub fn value() {}\n").unwrap();
        fs::write(
            root.join("dep/Cargo.toml"),
            "[package]\nname='shape-dep'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(root.join("dep/src/lib.rs"), "pub fn dependency() {}\n").unwrap();

        let shape = super::inspect(&root, &["Cargo.toml".into()]).unwrap();
        assert_eq!(shape.rust_source_files, 2);
        assert_eq!(shape.rust_lines, 2);
        assert_eq!(shape.manifests[0].selected_packages[0].name, "shape-root");
        let _ = fs::remove_dir_all(root);
    }
}
