use std::{path::Path, process::Command};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

const RELOCATABLE_INCREMENTAL_MIN_MINOR: u32 = 98;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustcChannel {
    Stable,
    Beta,
    Nightly,
    Dev,
}

#[derive(Debug, Clone, Serialize)]
pub struct RustcInfo {
    pub release: String,
    pub channel: RustcChannel,
    pub relocatable_incremental_supported: bool,
}

impl RustcInfo {
    pub fn accepts_unstable_flags(&self) -> bool {
        matches!(self.channel, RustcChannel::Nightly | RustcChannel::Dev)
    }
}

pub fn rustc_info(workspace: &Path) -> Result<RustcInfo> {
    let output = Command::new("rustc")
        .current_dir(workspace)
        .arg("-vV")
        .output()
        .with_context(|| format!("failed to inspect rustc in {}", workspace.display()))?;
    if !output.status.success() {
        return Err(anyhow!("rustc -vV failed with {}", output.status));
    }
    parse_rustc_verbose_version(&String::from_utf8_lossy(&output.stdout))
}

fn parse_rustc_verbose_version(text: &str) -> Result<RustcInfo> {
    let release = text
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .ok_or_else(|| anyhow!("rustc -vV did not report a release"))?
        .trim()
        .to_owned();

    let channel = if release.contains("nightly") {
        RustcChannel::Nightly
    } else if release.contains("beta") {
        RustcChannel::Beta
    } else if release.contains("dev") {
        RustcChannel::Dev
    } else {
        RustcChannel::Stable
    };

    let version = release.split('-').next().unwrap_or(&release);
    let mut components = version.split('.');
    let major = components
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or_else(|| anyhow!("could not parse rustc release {release}"))?;
    let minor = components
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or_else(|| anyhow!("could not parse rustc release {release}"))?;
    let relocatable_incremental_supported =
        major > 1 || (major == 1 && minor >= RELOCATABLE_INCREMENTAL_MIN_MINOR);

    Ok(RustcInfo {
        release,
        channel,
        relocatable_incremental_supported,
    })
}

#[cfg(test)]
mod tests {
    use super::{RustcChannel, parse_rustc_verbose_version};

    #[test]
    fn identifies_relocation_capability_and_channel() {
        let stable = parse_rustc_verbose_version("release: 1.98.0\n").unwrap();
        assert_eq!(stable.channel, RustcChannel::Stable);
        assert!(stable.relocatable_incremental_supported);

        let old = parse_rustc_verbose_version("release: 1.94.0\n").unwrap();
        assert!(!old.relocatable_incremental_supported);

        let nightly = parse_rustc_verbose_version("release: 1.99.0-nightly\n").unwrap();
        assert_eq!(nightly.channel, RustcChannel::Nightly);
        assert!(nightly.accepts_unstable_flags());
    }
}
