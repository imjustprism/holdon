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
    /// `[[check]]` blocks appended to `targets` in file order. Each block
    /// names a single readiness target and may carry future per-check
    /// overrides. Only `name` and `target` are recognised today; other keys
    /// are rejected at parse time so a typo in a future override key cannot
    /// silently degrade to the global setting.
    #[serde(default)]
    pub check: Vec<CheckEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckEntry {
    /// Optional label printed in the banner. Empty strings are rejected
    /// so an accidental `name = ""` cannot disable labelling silently.
    pub name: Option<String>,
    /// Target string in the same syntax as positional CLI arguments.
    pub target: String,
    /// Per-target direction: `"up"` (wait until ready, default) or
    /// `"down"` (wait until NOT ready). Lets a single run mix
    /// must-be-ready and must-be-down targets, which the global
    /// `--reverse` flag cannot express.
    pub direction: Option<String>,
    /// Initial retry interval applied before exponential backoff
    /// doubling, e.g. `"500ms"`. Overrides the global `interval` for
    /// this one target.
    pub interval: Option<String>,
    /// Per-attempt wall-clock budget for one probe, e.g. `"30s"`.
    /// Overrides the global `attempt_timeout` for this one target.
    pub attempt_timeout: Option<String>,
    /// Consecutive-success threshold before the target is considered
    /// satisfied. Overrides the global `success_threshold` for this
    /// one target. Values < 1 are rejected at parse time.
    pub success_threshold: Option<u32>,
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
    /// One entry per target in `targets`, in the same order. `None`
    /// for positional CLI targets and `targets = [...]` entries that
    /// carry no label, `Some` for `[[check]]` blocks with `name`.
    pub names: Vec<Option<String>>,
    /// One entry per target in `targets`. `None` means "fall back to
    /// the global direction", `Some(true)` flips polarity (wait for
    /// the target to be NOT ready).
    pub reverse_per_target: Vec<Option<bool>>,
    /// One entry per target in `targets`. Each tuple holds optional
    /// per-target overrides for `interval`, `attempt_timeout`, and
    /// `success_threshold`. Indices align with `targets`.
    pub overrides_per_target: Vec<PerTargetOverride>,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PerTargetOverride {
    pub interval: Option<Duration>,
    pub attempt_timeout: Option<Duration>,
    pub success_threshold: Option<u32>,
}

impl PerTargetOverride {
    pub(crate) const fn is_some(&self) -> bool {
        self.interval.is_some()
            || self.attempt_timeout.is_some()
            || self.success_threshold.is_some()
    }
}

pub(crate) fn load(explicit: Option<&Path>) -> Result<Resolved> {
    let (path, was_auto_detected) = match explicit {
        Some(p) => (Some(p.to_path_buf()), false),
        None => (auto_detect(), true),
    };
    let Some(path) = path else {
        return Ok(Resolved::default());
    };
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    let raw: ConfigFile = if was_auto_detected {
        // Strip the toml crate's source-snippet from the error chain so an
        // auto-loaded file (which the user may not have intended us to read)
        // cannot leak its contents into our diagnostics.
        toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("parsing TOML in {}: {}", path.display(), e.message()))?
    } else {
        toml::from_str(&contents).with_context(|| format!("parsing TOML in {}", path.display()))?
    };
    parse_durations(raw, &path)
}

/// Look for a `holdon.toml` or `.holdon.toml` regular file in the current
/// working directory. Symlinks are intentionally rejected so a hostile CWD
/// cannot redirect us to read another file (for example `/etc/passwd`) and
/// then surface its content in a parse error message.
fn auto_detect() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    for name in AUTO_DETECT_NAMES {
        let candidate = cwd.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&candidate) else {
            continue;
        };
        if meta.file_type().is_file() {
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
    let mut targets = raw.targets;
    let mut names: Vec<Option<String>> = std::iter::repeat_n(None, targets.len()).collect();
    let mut reverse_per_target: Vec<Option<bool>> =
        std::iter::repeat_n(None, targets.len()).collect();
    let mut overrides_per_target: Vec<PerTargetOverride> =
        std::iter::repeat_n(PerTargetOverride::default(), targets.len()).collect();
    for (i, entry) in raw.check.into_iter().enumerate() {
        let one = i + 1;
        if entry.target.trim().is_empty() {
            bail!(
                "{}: [[check]] #{i} has an empty target",
                path.display(),
                i = one
            );
        }
        if entry.name.as_deref().is_some_and(|s| s.trim().is_empty()) {
            bail!(
                "{}: [[check]] #{i} has an empty name (omit the field or set a value)",
                path.display(),
                i = one
            );
        }
        let direction = match entry.direction.as_deref().map(str::to_ascii_lowercase) {
            None => None,
            Some(ref s) if s == "up" || s == "wait" => Some(false),
            Some(ref s) if s == "down" || s == "reverse" => Some(true),
            Some(s) => {
                bail!(
                    "{}: [[check]] #{i} direction `{s}` invalid (expected `up` or `down`)",
                    path.display(),
                    i = one
                );
            }
        };
        let entry_interval = dur(
            &format!("[[check]] #{one} interval"),
            entry.interval.clone(),
        )?;
        let entry_attempt_timeout = dur(
            &format!("[[check]] #{one} attempt_timeout"),
            entry.attempt_timeout.clone(),
        )?;
        if entry.success_threshold == Some(0) {
            bail!(
                "{}: [[check]] #{i} success_threshold must be >= 1",
                path.display(),
                i = one
            );
        }
        let override_entry = PerTargetOverride {
            interval: entry_interval,
            attempt_timeout: entry_attempt_timeout,
            success_threshold: entry.success_threshold,
        };
        targets.push(entry.target);
        names.push(entry.name);
        reverse_per_target.push(direction);
        overrides_per_target.push(override_entry);
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
        targets,
        names,
        reverse_per_target,
        overrides_per_target,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn parse(content: &str) -> Result<Resolved> {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.as_file().write_all(content.as_bytes()).unwrap();
        load(Some(tmp.path()))
    }

    #[test]
    fn legacy_targets_array_still_works() {
        let r = parse("targets = [\":5432\", \"http://a/b\"]\n").unwrap();
        assert_eq!(r.targets, vec![":5432", "http://a/b"]);
        assert_eq!(r.names, vec![None, None]);
    }

    #[test]
    fn check_blocks_append_in_file_order() {
        let r = parse(
            "targets = [\":1\"]\n\
             [[check]]\n\
             name = \"db\"\n\
             target = \"postgres://db:5432\"\n\
             [[check]]\n\
             target = \"http://api/health\"\n",
        )
        .unwrap();
        assert_eq!(
            r.targets,
            vec![":1", "postgres://db:5432", "http://api/health"]
        );
        assert_eq!(r.names, vec![None, Some("db".to_owned()), None]);
    }

    #[test]
    fn empty_check_target_rejected() {
        let err = parse("[[check]]\ntarget = \"\"\n").unwrap_err().to_string();
        assert!(err.contains("empty target"), "got: {err}");
    }

    #[test]
    fn empty_check_name_rejected() {
        let err = parse("[[check]]\nname = \"\"\ntarget = \":1\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty name"), "got: {err}");
    }

    #[test]
    fn whitespace_only_check_name_rejected() {
        let err = parse("[[check]]\nname = \"   \"\ntarget = \":1\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty name"), "got: {err}");
    }

    #[test]
    fn unknown_check_field_rejected() {
        let err = parse("[[check]]\ntarget = \":1\"\nfuture = true\n").unwrap_err();
        let full = format!("{err:#}");
        assert!(full.contains("future"), "got: {full}");
    }

    #[test]
    fn check_direction_up_parses_as_wait() {
        let r = parse("[[check]]\ntarget = \":1\"\ndirection = \"up\"\n").unwrap();
        assert_eq!(r.reverse_per_target, vec![Some(false)]);
    }

    #[test]
    fn check_direction_down_parses_as_reverse() {
        let r = parse("[[check]]\ntarget = \":1\"\ndirection = \"down\"\n").unwrap();
        assert_eq!(r.reverse_per_target, vec![Some(true)]);
    }

    #[test]
    fn check_direction_case_insensitive() {
        let r = parse("[[check]]\ntarget = \":1\"\ndirection = \"DOWN\"\n").unwrap();
        assert_eq!(r.reverse_per_target, vec![Some(true)]);
    }

    #[test]
    fn check_direction_aliases() {
        let up = parse("[[check]]\ntarget = \":1\"\ndirection = \"wait\"\n").unwrap();
        assert_eq!(up.reverse_per_target, vec![Some(false)]);
        let down = parse("[[check]]\ntarget = \":1\"\ndirection = \"reverse\"\n").unwrap();
        assert_eq!(down.reverse_per_target, vec![Some(true)]);
    }

    #[test]
    fn check_direction_invalid_rejected() {
        let err = parse("[[check]]\ntarget = \":1\"\ndirection = \"sideways\"\n").unwrap_err();
        let full = format!("{err:#}");
        assert!(full.contains("sideways"), "got: {full}");
    }

    #[test]
    fn check_interval_override_parsed() {
        let r = parse("[[check]]\ntarget = \":1\"\ninterval = \"500ms\"\n").unwrap();
        assert_eq!(r.overrides_per_target.len(), 1);
        assert_eq!(
            r.overrides_per_target[0].interval,
            Some(Duration::from_millis(500))
        );
        assert!(r.overrides_per_target[0].attempt_timeout.is_none());
        assert!(r.overrides_per_target[0].success_threshold.is_none());
    }

    #[test]
    fn check_attempt_timeout_override_parsed() {
        let r = parse("[[check]]\ntarget = \":1\"\nattempt_timeout = \"30s\"\n").unwrap();
        assert_eq!(
            r.overrides_per_target[0].attempt_timeout,
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn check_success_threshold_override_parsed() {
        let r = parse("[[check]]\ntarget = \":1\"\nsuccess_threshold = 3\n").unwrap();
        assert_eq!(r.overrides_per_target[0].success_threshold, Some(3));
    }

    #[test]
    fn check_zero_success_threshold_rejected() {
        let err = parse("[[check]]\ntarget = \":1\"\nsuccess_threshold = 0\n").unwrap_err();
        let full = format!("{err:#}");
        assert!(
            full.contains("success_threshold must be >= 1"),
            "got: {full}"
        );
    }

    #[test]
    fn check_bad_duration_rejected() {
        let err = parse("[[check]]\ntarget = \":1\"\ninterval = \"forever\"\n").unwrap_err();
        let full = format!("{err:#}");
        assert!(full.contains("invalid"), "got: {full}");
    }

    #[test]
    fn check_no_overrides_yields_empty_per_target() {
        let r = parse("[[check]]\ntarget = \":1\"\n").unwrap();
        assert!(!r.overrides_per_target[0].is_some());
    }

    #[test]
    fn check_missing_direction_inherits_global() {
        let r = parse(
            "targets = [\":1\"]\n\
             [[check]]\n\
             target = \":2\"\n",
        )
        .unwrap();
        assert_eq!(r.reverse_per_target, vec![None, None]);
    }
}
