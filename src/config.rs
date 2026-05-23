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
    #[serde(default)]
    pub check: Vec<CheckEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckEntry {
    pub name: Option<String>,
    pub target: String,
    pub direction: Option<String>,
    pub interval: Option<String>,
    pub attempt_timeout: Option<String>,
    pub success_threshold: Option<u32>,
    #[serde(default)]
    pub after: Vec<String>,
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
    pub names: Vec<Option<String>>,
    pub reverse_per_target: Vec<Option<bool>>,
    pub overrides_per_target: Vec<PerTargetOverride>,
    pub prereqs_per_target: Vec<Vec<usize>>,
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

impl From<PerTargetOverride> for holdon::TargetOverrides {
    fn from(o: PerTargetOverride) -> Self {
        let mut t = Self::default();
        t.interval = o.interval;
        t.attempt_timeout = o.attempt_timeout;
        t.success_threshold = o.success_threshold;
        t
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
        toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("parsing TOML in {}: {}", path.display(), e.message()))?
    } else {
        toml::from_str(&contents).with_context(|| format!("parsing TOML in {}", path.display()))?
    };
    parse_durations(raw, &path)
}

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

#[allow(clippy::too_many_lines)]
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
    let mut prereqs_per_target: Vec<Vec<usize>> =
        std::iter::repeat_n(Vec::new(), targets.len()).collect();
    let mut after_refs: Vec<Vec<String>> = std::iter::repeat_n(Vec::new(), targets.len()).collect();
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
        for dep in &entry.after {
            if dep.trim().is_empty() {
                bail!(
                    "{}: [[check]] #{i} has an empty `after` entry",
                    path.display(),
                    i = one
                );
            }
        }
        targets.push(entry.target);
        names.push(entry.name);
        reverse_per_target.push(direction);
        overrides_per_target.push(override_entry);
        prereqs_per_target.push(Vec::new());
        after_refs.push(entry.after);
    }

    if after_refs.iter().any(|a| !a.is_empty()) {
        let mut name_to_idx: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for (idx, n) in names.iter().enumerate() {
            if let Some(s) = n {
                if name_to_idx.insert(s.as_str(), idx).is_some() {
                    bail!(
                        "{}: duplicate check name `{s}` (after-references need unique names)",
                        path.display()
                    );
                }
            }
        }
        for (idx, deps) in after_refs.iter().enumerate() {
            for dep in deps {
                let Some(&dep_idx) = name_to_idx.get(dep.as_str()) else {
                    bail!(
                        "{}: [[check]] index {idx} references unknown after = `{dep}`",
                        path.display()
                    );
                };
                if dep_idx == idx {
                    bail!(
                        "{}: [[check]] `{dep}` cannot depend on itself",
                        path.display()
                    );
                }
                prereqs_per_target[idx].push(dep_idx);
            }
        }
        detect_cycle(&prereqs_per_target, path)?;
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
        prereqs_per_target,
    })
}

fn detect_cycle(prereqs: &[Vec<usize>], path: &Path) -> Result<()> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color = vec![WHITE; prereqs.len()];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for start in 0..prereqs.len() {
        if color[start] != WHITE {
            continue;
        }
        stack.push((start, 0));
        color[start] = GRAY;
        while let Some(&(node, idx)) = stack.last() {
            if idx >= prereqs[node].len() {
                color[node] = BLACK;
                stack.pop();
                continue;
            }
            let next = prereqs[node][idx];
            if let Some(top) = stack.last_mut() {
                top.1 = idx + 1;
            }
            match color[next] {
                WHITE => {
                    color[next] = GRAY;
                    stack.push((next, 0));
                }
                GRAY => {
                    bail!(
                        "{}: cyclic check dependency detected involving index {next}",
                        path.display()
                    );
                }
                _ => {}
            }
        }
    }
    Ok(())
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
    fn check_after_resolves_to_prereqs() {
        let r = parse(
            "[[check]]\n\
             name = \"db\"\n\
             target = \":1\"\n\
             [[check]]\n\
             name = \"api\"\n\
             target = \":2\"\n\
             after = [\"db\"]\n",
        )
        .unwrap();
        assert_eq!(r.prereqs_per_target.len(), 2);
        assert!(r.prereqs_per_target[0].is_empty());
        assert_eq!(r.prereqs_per_target[1], vec![0]);
    }

    #[test]
    fn check_after_unknown_name_rejected() {
        let err = parse(
            "[[check]]\n\
             name = \"api\"\n\
             target = \":2\"\n\
             after = [\"nope\"]\n",
        )
        .unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("unknown after"), "got: {s}");
    }

    #[test]
    fn check_after_self_reference_rejected() {
        let err = parse(
            "[[check]]\n\
             name = \"a\"\n\
             target = \":1\"\n\
             after = [\"a\"]\n",
        )
        .unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("cannot depend on itself"), "got: {s}");
    }

    #[test]
    fn check_after_cycle_rejected() {
        let err = parse(
            "[[check]]\n\
             name = \"a\"\n\
             target = \":1\"\n\
             after = [\"b\"]\n\
             [[check]]\n\
             name = \"b\"\n\
             target = \":2\"\n\
             after = [\"a\"]\n",
        )
        .unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("cyclic"), "got: {s}");
    }

    #[test]
    fn check_after_empty_entry_rejected() {
        let err = parse(
            "[[check]]\n\
             name = \"a\"\n\
             target = \":1\"\n\
             after = [\"\"]\n",
        )
        .unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("empty `after`"), "got: {s}");
    }

    #[test]
    fn check_after_duplicate_names_rejected() {
        let err = parse(
            "[[check]]\n\
             name = \"a\"\n\
             target = \":1\"\n\
             [[check]]\n\
             name = \"a\"\n\
             target = \":2\"\n\
             after = [\"a\"]\n",
        )
        .unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("duplicate check name"), "got: {s}");
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
