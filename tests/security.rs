#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]

use holdon::util::{format_error_chain, redact_in, sanitize_for_terminal};
use holdon::{Hostname, Target};

#[test]
fn hostname_rejects_control_bytes() {
    assert!(Hostname::new("good-host").is_ok());
    assert!(Hostname::new("with\x00nul").is_err());
    assert!(Hostname::new("with\x1bescape").is_err());
    assert!(Hostname::new("with\nnewline").is_err());
}

#[test]
fn hostname_rejects_empty_and_too_long() {
    assert!(Hostname::new("").is_err());
    let too_long = "a".repeat(254);
    assert!(Hostname::new(too_long).is_err());
    let exactly_max = "a".repeat(253);
    assert!(Hostname::new(exactly_max).is_ok());
}

#[test]
fn hostname_via_target_parse_propagates_validation() {
    // Embedded NUL in the host portion of a target string should fail.
    let bad = "host\x00name:5432";
    assert!(bad.parse::<Target>().is_err());
}

#[test]
fn unc_path_refused_at_parse() {
    assert!("file://attacker.com/share/x".parse::<Target>().is_err());
    assert!("file:////share/path".parse::<Target>().is_err());
}

#[test]
fn target_display_redacts_password_postgres() {
    let t: Target = "postgres://user:hunter2@db:5432/x".parse().unwrap();
    let s = t.to_string();
    assert!(!s.contains("hunter2"), "leaked: {s}");
    assert!(s.contains("***"), "no marker: {s}");
}

#[test]
fn target_display_redacts_password_redis() {
    let t: Target = "rediss://default:topsecret@cache:6379/0".parse().unwrap();
    assert!(!t.to_string().contains("topsecret"));
}

#[test]
fn target_debug_redacts_password() {
    let t: Target = "postgres://app:supersecret@db/x".parse().unwrap();
    let s = format!("{t:?}");
    assert!(!s.contains("supersecret"), "Debug leaked: {s}");
    assert!(s.contains("***"), "Debug no marker: {s}");
}

#[test]
fn sanitize_strips_ansi_escape_csi() {
    let evil = "before\x1b[31m\x1b[1mTAKEOVER\x1b[0mafter";
    let safe = sanitize_for_terminal(evil);
    assert!(!safe.contains('\x1b'), "ESC leaked: {safe:?}");
    assert!(safe.contains("before"));
    assert!(safe.contains("after"));
}

#[test]
fn sanitize_strips_carriage_return_overstrike() {
    let evil = "real_line\rOVERWRITTEN";
    let safe = sanitize_for_terminal(evil);
    assert!(!safe.contains('\r'));
}

#[test]
fn sanitize_strips_bell_and_del() {
    let evil = "ding\x07del\x7fend";
    let safe = sanitize_for_terminal(evil);
    assert!(!safe.contains('\x07'));
    assert!(!safe.contains('\x7f'));
}

#[test]
fn sanitize_strips_osc_title_rewrite() {
    let evil = "\x1b]0;EvilTitle\x07payload";
    let safe = sanitize_for_terminal(evil);
    assert!(!safe.contains('\x1b'));
    assert!(!safe.contains('\x07'));
    assert!(safe.contains("payload"));
}

#[test]
fn sanitize_keeps_printable_unicode() {
    let s = "ascii ▎ ✓ ✗ → · ●";
    assert_eq!(sanitize_for_terminal(s), s);
}

#[test]
fn sanitize_keeps_tab_and_newline() {
    assert_eq!(sanitize_for_terminal("a\tb\nc"), "a\tb\nc");
}

#[test]
fn redact_replaces_all_occurrences() {
    let s = redact_in("hunter2 then hunter2 and hunter2", "hunter2");
    assert_eq!(s, "*** then *** and ***");
}

#[test]
fn redact_empty_secret_is_noop() {
    assert_eq!(redact_in("nothing", ""), "nothing");
}

#[test]
fn format_error_chain_walks_sources_and_sanitizes() {
    use std::error::Error;
    use std::fmt;

    #[derive(Debug)]
    struct Inner;
    impl fmt::Display for Inner {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "ROOT_CAUSE\x1b[31mEVIL")
        }
    }
    impl Error for Inner {}

    #[derive(Debug)]
    struct Outer(Inner);
    impl fmt::Display for Outer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "outer")
        }
    }
    impl Error for Outer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    let s = format_error_chain(&Outer(Inner));
    assert!(s.contains("outer"));
    assert!(s.contains("ROOT_CAUSE"));
    assert!(!s.contains('\x1b'), "ANSI leaked from source: {s}");
}

#[test]
fn ambiguous_ipv6_without_brackets_rejected() {
    assert!("::1:5432".parse::<Target>().is_err());
    assert!("2001:db8::1:5432".parse::<Target>().is_err());
}

#[test]
fn mode_absent_strict_query_parsing() {
    use holdon::target::FileMode;

    #[cfg(windows)]
    let evil = "file:///C:/?log=mode=absent";
    #[cfg(not(windows))]
    let evil = "file:///?log=mode=absent";
    let t: Target = evil.parse().unwrap();
    match t {
        Target::File { mode, .. } => assert_eq!(mode, FileMode::Present),
        _ => panic!("expected File"),
    }

    #[cfg(windows)]
    let real = "file:///C:/?mode=absent";
    #[cfg(not(windows))]
    let real = "file:///?mode=absent";
    let t: Target = real.parse().unwrap();
    match t {
        Target::File { mode, .. } => assert_eq!(mode, FileMode::Absent),
        _ => panic!("expected File"),
    }
}
