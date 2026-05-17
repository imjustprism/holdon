use std::time::Duration;

const MICROS_PER_SEC: f64 = 1_000_000.0;
const MICROS_PER_MS: f64 = 1_000.0;
const MICROS_PER_MIN: f64 = 60.0 * MICROS_PER_SEC;
const MICROS_PER_HOUR: f64 = 60.0 * MICROS_PER_MIN;
const MAX_F64_MICROS: f64 = 9_223_372_036_854_775_000.0;
const FMT_DUR_SECS_BOUNDARY_MS: u128 = 1_000;

#[must_use]
pub fn format_error_chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut cur = err.source();
    while let Some(src) = cur {
        out.push_str(": ");
        out.push_str(&src.to_string());
        cur = src.source();
    }
    sanitize_for_terminal(&out)
}

#[must_use]
pub fn sanitize_for_terminal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' | '\n' => out.push(c),
            c if (c as u32) < 0x20 || c == '\x7f' => out.push('\u{fffd}'),
            _ => out.push(c),
        }
    }
    out
}

#[must_use]
pub fn redact_in(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return redact_userinfo(text);
    }
    let decoded = percent_encoding::percent_decode_str(secret)
        .decode_utf8()
        .ok();
    let mut out = text.replace(secret, "***");
    if let Some(d) = decoded.as_deref() {
        if d != secret {
            out = out.replace(d, "***");
        }
    }
    redact_userinfo(&out)
}

/// Replace any `scheme://user:password@` URL userinfo with `scheme://***:***@`.
///
/// Defense in depth: catches password leaks even when the secret string is
/// not known to the caller (e.g. a driver echoes a percent-decoded form, or
/// a different secret slips through).
#[must_use]
pub fn redact_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let bytes = text.as_bytes();
    while let Some(rel) = text[cursor..].find("://") {
        let scheme_end = cursor + rel + 3;
        out.push_str(&text[cursor..scheme_end]);
        let segment_end = bytes[scheme_end..]
            .iter()
            .position(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'"' | b'\'' | b'<' | b'>'))
            .map_or(text.len(), |off| scheme_end + off);
        let segment = &text[scheme_end..segment_end];
        let host_start = segment.find('/').unwrap_or(segment.len());
        let authority = &segment[..host_start];
        if let Some(at) = authority.rfind('@') {
            if authority[..at].contains(':') {
                out.push_str("***:***@");
            } else {
                out.push_str(&authority[..at]);
                out.push('@');
            }
            out.push_str(&authority[at + 1..]);
            out.push_str(&segment[host_start..]);
        } else {
            out.push_str(segment);
        }
        cursor = segment_end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[must_use]
pub fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[must_use]
pub fn fmt_dur(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < FMT_DUR_SECS_BOUNDARY_MS {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    match s.to_ascii_lowercase().as_str() {
        "never" | "infinite" | "none" => return Ok(Duration::MAX),
        _ => {}
    }
    let (num, unit) = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .map_or((s, ""), |i| s.split_at(i));
    let n: f64 = num
        .parse()
        .map_err(|e| format!("invalid number `{num}`: {e}"))?;
    if !n.is_finite() || n < 0.0 {
        return Err(format!(
            "invalid duration `{s}`: must be finite and non-negative"
        ));
    }
    let mult_us: f64 = match unit.trim() {
        "" | "s" => MICROS_PER_SEC,
        "ms" => MICROS_PER_MS,
        "us" | "µs" => 1.0,
        "m" => MICROS_PER_MIN,
        "h" => MICROS_PER_HOUR,
        other => return Err(format!("unknown unit `{other}` (use ms, s, m, h)")),
    };
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss
    )]
    let micros = (n * mult_us).clamp(0.0, MAX_F64_MICROS) as u64;
    Ok(Duration::from_micros(micros))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_seconds_default() {
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn parses_units() {
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("500us").unwrap(), Duration::from_micros(500));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn rejects_invalid() {
        assert!(parse_duration("inf").is_err());
        assert!(parse_duration("NaN").is_err());
        assert!(parse_duration("-5s").is_err());
        assert!(parse_duration("5x").is_err());
    }

    #[test]
    fn parses_unlimited_sentinels() {
        assert_eq!(parse_duration("never").unwrap(), Duration::MAX);
        assert_eq!(parse_duration("Never").unwrap(), Duration::MAX);
        assert_eq!(parse_duration("infinite").unwrap(), Duration::MAX);
        assert_eq!(parse_duration("none").unwrap(), Duration::MAX);
    }

    #[test]
    fn parses_zero_as_zero_so_holdon_interval_zero_means_no_delay() {
        assert_eq!(parse_duration("0").unwrap(), Duration::ZERO);
        assert_eq!(parse_duration("0s").unwrap(), Duration::ZERO);
    }

    #[test]
    fn fmt_dur_human() {
        assert_eq!(fmt_dur(Duration::from_millis(42)), "42ms");
        assert_eq!(fmt_dur(Duration::from_millis(1500)), "1.5s");
    }

    #[test]
    fn sanitize_strips_escape_and_control() {
        let evil = "ok\x1b[31mred\x1b[0m\x07\rrest";
        let safe = sanitize_for_terminal(evil);
        assert!(!safe.contains('\x1b'));
        assert!(!safe.contains('\x07'));
        assert!(!safe.contains('\r'));
        assert!(safe.contains("ok"));
    }

    #[test]
    fn sanitize_keeps_tab_and_newline() {
        assert_eq!(sanitize_for_terminal("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn redact_replaces_secret() {
        assert_eq!(
            redact_in("user:hunter2@host", "hunter2"),
            "user:***@host"
        );
        assert_eq!(redact_in("none", ""), "none");
    }

    #[test]
    fn redact_userinfo_strips_password_in_url() {
        assert_eq!(
            redact_userinfo("connect failed at postgres://u:p@h:5432/db now"),
            "connect failed at postgres://***:***@h:5432/db now"
        );
    }

    #[test]
    fn redact_userinfo_keeps_username_only() {
        assert_eq!(
            redact_userinfo("mongodb://user@h/admin"),
            "mongodb://user@h/admin"
        );
    }

    #[test]
    fn redact_userinfo_no_op_without_authority() {
        assert_eq!(redact_userinfo("plain text"), "plain text");
        assert_eq!(redact_userinfo("/path/only"), "/path/only");
    }

    #[test]
    fn redact_userinfo_handles_multiple_urls() {
        let s = "from postgres://u:p@a/b to redis://x:y@c";
        assert_eq!(
            redact_userinfo(s),
            "from postgres://***:***@a/b to redis://***:***@c"
        );
    }

    #[test]
    fn redact_in_decodes_percent_encoded_password() {
        let raw = "p%40ss";
        let leaked = "error: auth as 'p@ss' failed";
        assert_eq!(
            redact_in(leaked, raw),
            "error: auth as '***' failed"
        );
    }

    #[test]
    fn cli_defaults_match_runner_constants() {
        use crate::RunnerConfig;
        assert_eq!(
            parse_duration("30s").unwrap(),
            RunnerConfig::DEFAULT_OVERALL_TIMEOUT
        );
        assert_eq!(
            parse_duration("100ms").unwrap(),
            RunnerConfig::DEFAULT_INITIAL_INTERVAL
        );
        assert_eq!(
            parse_duration("2s").unwrap(),
            RunnerConfig::DEFAULT_MAX_INTERVAL
        );
        assert_eq!(
            parse_duration("0s").unwrap(),
            RunnerConfig::DEFAULT_INITIAL_DELAY
        );
        assert_eq!(
            parse_duration("5s").unwrap(),
            RunnerConfig::DEFAULT_ATTEMPT_TIMEOUT
        );
    }
}
