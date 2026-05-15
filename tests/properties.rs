#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use holdon::Target;
use holdon::parse_duration;
use proptest::prelude::*;

proptest! {
    #[test]
    fn parse_duration_never_panics(s in "\\PC*") {
        let _ = parse_duration(&s);
    }

    #[test]
    fn parse_duration_ms_roundtrip(ms in 0u64..1_000_000_000) {
        let s = format!("{ms}ms");
        let d = parse_duration(&s).unwrap();
        prop_assert_eq!(d, Duration::from_millis(ms));
    }

    #[test]
    fn parse_duration_bare_is_seconds(secs in 0u32..86_400) {
        let d = parse_duration(&secs.to_string()).unwrap();
        prop_assert_eq!(d, Duration::from_secs(u64::from(secs)));
    }

    #[test]
    fn parse_duration_rejects_negative(n in 1u32..1_000_000) {
        let s1 = format!("-{n}s");
        let s2 = format!("-{n}ms");
        prop_assert!(parse_duration(&s1).is_err());
        prop_assert!(parse_duration(&s2).is_err());
    }
}

proptest! {
    #[test]
    fn target_parse_never_panics(s in "\\PC*") {
        let _ = s.parse::<Target>();
    }

    #[test]
    fn tcp_target_roundtrips(
        host in "[a-z][a-z0-9-]{0,30}",
        port in 1u16..65535,
    ) {
        let s = format!("{host}:{port}");
        let t: Target = s.parse().unwrap();
        let display = t.to_string();
        let t2: Target = display.parse().unwrap();
        prop_assert_eq!(t.to_string(), t2.to_string());
    }

    #[test]
    fn shorthand_port_means_localhost(port in 1u16..65535) {
        let s = format!(":{port}");
        let t: Target = s.parse().unwrap();
        prop_assert_eq!(t.to_string(), format!("tcp://localhost:{port}"));
    }

    #[test]
    fn ipv6_bracketed_parses(
        a in 0u16..=0xffff,
        b in 0u16..=0xffff,
        port in 1u16..65535,
    ) {
        let s = format!("[{a:x}::{b:x}]:{port}");
        let t: Target = s.parse().unwrap();
        let is_tcp = matches!(t, Target::Tcp { .. });
        prop_assert!(is_tcp);
    }

    #[test]
    fn ambiguous_ipv6_rejected(
        a in 0u16..=0xffff,
        b in 0u16..=0xffff,
        port in 1u16..65535,
    ) {
        let s = format!("{a:x}:{b:x}:{port}");
        prop_assert!(s.parse::<Target>().is_err());
    }
}
