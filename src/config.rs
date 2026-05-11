use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const AUTO_DETECT_NAMES: &[&str] = &["holdon.toml", ".holdon.toml"];

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigFile {
    pub interval: Option<String>,
    pub timeout: Option<String>,
    pub max_interval: Option<String>,
    pub initial_delay: Option<String>,
    pub attempt_timeout: Option<String>,
    pub success_threshold: Option<u32>,
    pub jitter: Option<bool>,
    pub sequential: Option<bool>,
    pub reverse: Option<bool>,
    pub once: Option<bool>,
    pub at_least: Option<usize>,
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Resolved {
    pub interval: Option<Duration>,
    pub timeout: Option<Duration>,
    pub max_interval: Option<Duration>,
    pub initial_delay: Option<Duration>,
    pub attempt_timeout: Option<Duration>,
    pub success_threshold: Option<u32>,
    pub jitter: Option<bool>,
    pub sequential: Option<bool>,
    pub reverse: Option<bool>,
    pub once: Option<bool>,
    pub at_least: Option<usize>,
    pub targets: Vec<String>,
}

pub(crate) fn load(explicit: Option<&Path>) -> Result<Resolved> {
    let path = match explicit {
        Some(p) => Some(p.to_path_buf()),
        None => auto_detect(),
    };
    let Some(path) = path else {
        return Ok(Resolved::default());
    };
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    let raw: ConfigFile =
        toml::from_str(&contents).with_context(|| format!("parsing TOML in {}", path.display()))?;
    parse_durations(raw, &path)
}

fn auto_detect() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for name in AUTO_DETECT_NAMES {
        let candidate = cwd.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn parse_durations(raw: ConfigFile, path: &Path) -> Result<Resolved> {
    let dur = |field: &str, value: Option<String>| -> Result<Option<Duration>> {
        let Some(s) = value else { return Ok(None) };
        holdon::parse_duration(&s)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("{}: invalid {field} `{s}`: {e}", path.display()))
    };
    if raw.success_threshold == Some(0) {
        bail!("{}: success_threshold must be >= 1", path.display());
    }
    Ok(Resolved {
        interval: dur("interval", raw.interval)?,
        timeout: dur("timeout", raw.timeout)?,
        max_interval: dur("max_interval", raw.max_interval)?,
        initial_delay: dur("initial_delay", raw.initial_delay)?,
        attempt_timeout: dur("attempt_timeout", raw.attempt_timeout)?,
        success_threshold: raw.success_threshold,
        jitter: raw.jitter,
        sequential: raw.sequential,
        reverse: raw.reverse,
        once: raw.once,
        at_least: raw.at_least,
        targets: raw.targets,
    })
}
