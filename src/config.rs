use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG_FILE: &str = ".agents/.cargo-warm.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum PrimeMode {
    #[default]
    None,
    Rustc,
    Package,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum ClonePressure {
    #[default]
    Auto,
    Gentle,
    Fast,
    Max,
}

impl std::fmt::Display for ClonePressure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Gentle => "gentle",
            Self::Fast => "fast",
            Self::Max => "max",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CloneSettings {
    pub pressure: ClonePressure,
    pub workers: Option<usize>,
}

impl Default for CloneSettings {
    fn default() -> Self {
        Self {
            pressure: ClonePressure::Auto,
            workers: None,
        }
    }
}

impl CloneSettings {
    pub fn effective_workers(self, tasks: usize) -> usize {
        if tasks == 0 {
            return 1;
        }
        let available = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(4);
        let workers = self.workers.unwrap_or_else(|| match self.pressure {
            ClonePressure::Auto => (available / 4).clamp(1, 4),
            ClonePressure::Gentle => 1,
            ClonePressure::Fast => 4,
            ClonePressure::Max => available.saturating_mul(2).clamp(1, 16),
        });
        workers.clamp(1, tasks)
    }
}

impl std::fmt::Display for PrimeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::None => "none",
            Self::Rustc => "rustc",
            Self::Package => "package",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeedProfile {
    pub name: String,
    pub include_target: bool,
    pub copy_fallback: bool,
    pub freshness_rebase: bool,
    pub prime: PrimeMode,
    pub unstable_bootstrap: bool,
    pub seed_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveSeedConfig {
    pub config_path: Option<PathBuf>,
    pub profile: SeedProfile,
    pub clone: CloneSettings,
    pub manifests: Vec<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProjectConfig {
    version: Option<u32>,
    default_profile: Option<String>,
    manifests: Option<Vec<PathBuf>>,
    unstable_bootstrap: Option<bool>,
    seed_paths: Option<Vec<PathBuf>>,
    clone_pressure: Option<ClonePressure>,
    clone_workers: Option<usize>,
    #[serde(default)]
    profiles: BTreeMap<String, ProfileDefinition>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProfileDefinition {
    inherits: Option<String>,
    include_target: Option<bool>,
    copy_fallback: Option<bool>,
    freshness_rebase: Option<bool>,
    prime: Option<PrimeMode>,
    unstable_bootstrap: Option<bool>,
    seed_paths: Option<Vec<PathBuf>>,
}

#[derive(Debug, Default)]
pub struct SeedOverrides {
    pub profile: Option<String>,
    pub config: Option<PathBuf>,
    pub manifests: Vec<PathBuf>,
    pub include_target: bool,
    pub copy_fallback: bool,
    pub seed_paths: Vec<PathBuf>,
    pub disable_freshness_rebase: bool,
    pub legacy_prime: bool,
    pub prime_mode: Option<PrimeMode>,
    pub unstable_bootstrap: bool,
    pub clone_pressure: Option<ClonePressure>,
    pub clone_workers: Option<usize>,
}

pub fn resolve_seed(workspace: &Path, overrides: SeedOverrides) -> Result<EffectiveSeedConfig> {
    let (config_path, project) = load_project_config(workspace, overrides.config.as_deref())?;
    let selected_name = overrides
        .profile
        .clone()
        .or_else(|| project.default_profile.clone())
        .unwrap_or_else(|| "quick".to_string());
    let mut profile = resolve_profile(&selected_name, &project)?;

    if let Some(value) = project.unstable_bootstrap {
        profile.unstable_bootstrap = value;
    }
    if let Some(paths) = &project.seed_paths {
        append_unique(&mut profile.seed_paths, paths.iter().cloned());
    }

    if overrides.include_target {
        profile.include_target = true;
    }
    if overrides.copy_fallback {
        profile.copy_fallback = true;
    }
    if overrides.disable_freshness_rebase {
        profile.freshness_rebase = false;
    }
    if overrides.legacy_prime {
        profile.prime = PrimeMode::Package;
    }
    if let Some(mode) = overrides.prime_mode {
        profile.prime = mode;
    }
    if overrides.unstable_bootstrap {
        profile.unstable_bootstrap = true;
    }
    append_unique(&mut profile.seed_paths, overrides.seed_paths);

    let clone = CloneSettings {
        pressure: overrides
            .clone_pressure
            .or(project.clone_pressure)
            .unwrap_or_default(),
        workers: overrides.clone_workers.or(project.clone_workers),
    };
    if clone.workers == Some(0) {
        bail!("clone-workers must be at least 1");
    }

    let manifests = if !overrides.manifests.is_empty() {
        overrides.manifests
    } else if let Some(manifests) = project.manifests {
        if manifests.is_empty() {
            vec![PathBuf::from("Cargo.toml")]
        } else {
            manifests
        }
    } else {
        vec![PathBuf::from("Cargo.toml")]
    };

    Ok(EffectiveSeedConfig {
        config_path,
        profile,
        clone,
        manifests,
    })
}

pub fn available_profile_names(
    workspace: &Path,
    explicit_config: Option<&Path>,
) -> Result<Vec<String>> {
    let (_, project) = load_project_config(workspace, explicit_config)?;
    let mut names = vec![
        "quick".to_string(),
        "balanced".to_string(),
        "deep".to_string(),
    ];
    let mut custom: Vec<_> = project
        .profiles
        .keys()
        .filter(|name| !matches!(name.as_str(), "quick" | "balanced" | "deep"))
        .cloned()
        .collect();
    custom.sort();
    names.extend(custom);
    Ok(names)
}

fn built_in_profile(name: &str) -> Option<SeedProfile> {
    let prime = match name {
        "quick" => PrimeMode::None,
        "balanced" => PrimeMode::Rustc,
        "deep" => PrimeMode::Package,
        _ => return None,
    };
    Some(SeedProfile {
        name: name.to_string(),
        include_target: false,
        copy_fallback: false,
        freshness_rebase: true,
        prime,
        unstable_bootstrap: false,
        seed_paths: Vec::new(),
    })
}

fn resolve_profile(name: &str, project: &ProjectConfig) -> Result<SeedProfile> {
    let mut stack = Vec::new();
    resolve_profile_inner(name, project, &mut stack)
}

fn resolve_profile_inner(
    name: &str,
    project: &ProjectConfig,
    stack: &mut Vec<String>,
) -> Result<SeedProfile> {
    if stack.iter().any(|entry| entry == name) {
        stack.push(name.to_string());
        bail!(
            "cargo-warm profile inheritance cycle: {}",
            stack.join(" -> ")
        );
    }
    stack.push(name.to_string());

    let definition = project.profiles.get(name);
    let mut profile = if let Some(definition) = definition {
        if let Some(parent) = &definition.inherits {
            resolve_profile_inner(parent, project, stack)?
        } else if let Some(builtin) = built_in_profile(name) {
            builtin
        } else {
            built_in_profile("quick").expect("quick profile exists")
        }
    } else if let Some(builtin) = built_in_profile(name) {
        builtin
    } else {
        let mut known = available_names(project);
        known.sort();
        bail!(
            "unknown cargo-warm profile `{name}`; available profiles: {}",
            known.join(", ")
        );
    };

    if let Some(definition) = definition {
        apply_definition(&mut profile, definition);
    }
    profile.name = name.to_string();
    stack.pop();
    Ok(profile)
}

fn apply_definition(profile: &mut SeedProfile, definition: &ProfileDefinition) {
    if let Some(value) = definition.include_target {
        profile.include_target = value;
    }
    if let Some(value) = definition.copy_fallback {
        profile.copy_fallback = value;
    }
    if let Some(value) = definition.freshness_rebase {
        profile.freshness_rebase = value;
    }
    if let Some(value) = definition.prime {
        profile.prime = value;
    }
    if let Some(value) = definition.unstable_bootstrap {
        profile.unstable_bootstrap = value;
    }
    if let Some(paths) = &definition.seed_paths {
        append_unique(&mut profile.seed_paths, paths.iter().cloned());
    }
}

fn available_names(project: &ProjectConfig) -> Vec<String> {
    let mut names = vec![
        "quick".to_string(),
        "balanced".to_string(),
        "deep".to_string(),
    ];
    names.extend(project.profiles.keys().cloned());
    names
}

fn append_unique(target: &mut Vec<PathBuf>, values: impl IntoIterator<Item = PathBuf>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn load_project_config(
    workspace: &Path,
    explicit: Option<&Path>,
) -> Result<(Option<PathBuf>, ProjectConfig)> {
    let path = match explicit {
        Some(path) => Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace.join(path)
        }),
        None => discover_config(workspace)?,
    };
    let Some(path) = path else {
        return Ok((None, ProjectConfig::default()));
    };
    if !path.is_file() {
        bail!("cargo-warm config does not exist: {}", path.display());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read cargo-warm config {}", path.display()))?;
    let project: ProjectConfig = toml::from_str(&text)
        .with_context(|| format!("failed to parse cargo-warm config {}", path.display()))?;
    if let Some(version) = project.version
        && version != 1
    {
        bail!("unsupported cargo-warm config version {version}; expected version = 1");
    }
    Ok((Some(path), project))
}

fn discover_config(workspace: &Path) -> Result<Option<PathBuf>> {
    let git_root = Command::new("git")
        .current_dir(workspace)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(output) = git_root
        && output.status.success()
    {
        let root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        let candidate = root.join(DEFAULT_CONFIG_FILE);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    let candidate = workspace.join(DEFAULT_CONFIG_FILE);
    Ok(candidate.is_file().then_some(candidate))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{ClonePressure, PrimeMode, SeedOverrides, resolve_seed};

    fn fixture(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("cargo-warm-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn custom_profile_inherits_builtin_and_configures_manifests() {
        let root = fixture("inherits");
        fs::create_dir_all(root.join(".agents")).unwrap();
        fs::write(
            root.join(".agents/.cargo-warm.toml"),
            r#"
version = 1
default-profile = "agent"
manifests = ["app/Cargo.toml", "tools/Cargo.toml"]
unstable-bootstrap = true

[profiles.agent]
inherits = "deep"
seed-paths = ["native/cache"]
"#,
        )
        .unwrap();

        let resolved = resolve_seed(&root, SeedOverrides::default()).unwrap();
        assert_eq!(resolved.profile.name, "agent");
        assert_eq!(resolved.profile.prime, PrimeMode::Package);
        assert!(resolved.profile.unstable_bootstrap);
        assert_eq!(
            resolved.profile.seed_paths,
            [std::path::PathBuf::from("native/cache")]
        );
        assert_eq!(resolved.manifests.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cli_prime_mode_overrides_profile() {
        let root = fixture("override");
        let resolved = resolve_seed(
            &root,
            SeedOverrides {
                profile: Some("deep".to_string()),
                prime_mode: Some(PrimeMode::None),
                ..SeedOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(resolved.profile.prime, PrimeMode::None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clone_settings_are_project_level_and_orthogonal_to_profiles() {
        let root = fixture("clone-settings");
        fs::create_dir_all(root.join(".agents")).unwrap();
        fs::write(
            root.join(".agents/.cargo-warm.toml"),
            r#"
version = 1
default-profile = "deep"
clone-pressure = "fast"
clone-workers = 2
"#,
        )
        .unwrap();

        let resolved = resolve_seed(&root, SeedOverrides::default()).unwrap();
        assert_eq!(resolved.profile.name, "deep");
        assert_eq!(resolved.profile.prime, PrimeMode::Package);
        assert_eq!(resolved.clone.pressure, ClonePressure::Fast);
        assert_eq!(resolved.clone.workers, Some(2));
        assert_eq!(resolved.clone.effective_workers(8), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clone_worker_override_must_be_positive() {
        let root = fixture("clone-workers-zero");
        let error = resolve_seed(
            &root,
            SeedOverrides {
                clone_workers: Some(0),
                ..SeedOverrides::default()
            },
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("clone-workers must be at least 1")
        );
        let _ = fs::remove_dir_all(root);
    }
}
