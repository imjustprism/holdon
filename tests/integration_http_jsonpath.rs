#![cfg(feature = "http")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::manual_let_else
)]

mod common;

use std::sync::Once;
use std::time::Duration;

use common::run;
use holdon::Target;
use holdon::checker::http::{HttpConfig, set_global};
use holdon::runner::RunnerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static INIT: Once = Once::new();

fn install_expectations() {
    INIT.call_once(|| {
        let mut cfg = HttpConfig::defaults();
        cfg.jsonpath_expectations.push((
            serde_json_path::JsonPath::parse("$.status").unwrap(),
            "ok".to_owned(),
        ));
        cfg.jsonpath_expectations.push((
            serde_json_path::JsonPath::parse("$.items[1].name").unwrap(),
            "b".to_owned(),
        ));
        cfg.jsonpath_expectations.push((
            serde_json_path::JsonPath::parse("$.items[?(@.id == 7)].name").unwrap(),
            "seven".to_owned(),
        ));
        set_global(cfg);
    });
}

async fn spawn_json_server(body: String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = tokio::time::timeout(Duration::from_millis(500), sock.read(&mut buf)).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    port
}

fn quick(timeout_ms: u64) -> RunnerConfig {
    RunnerConfig::default()
        .timeout(Duration::from_millis(timeout_ms))
        .interval(Duration::from_millis(50))
        .once(true)
}

#[tokio::test(flavor = "multi_thread")]
async fn jsonpath_all_expectations_match() {
    install_expectations();
    let body = r#"{"status":"ok","items":[{"id":1,"name":"a"},{"id":2,"name":"b"},{"id":7,"name":"seven"}]}"#.to_owned();
    let port = spawn_json_server(body).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick(3000), vec![target]).await;
    assert!(report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn jsonpath_first_expectation_mismatch_fails() {
    install_expectations();
    let body = r#"{"status":"degraded","items":[{"id":1,"name":"a"},{"id":2,"name":"b"},{"id":7,"name":"seven"}]}"#
        .to_owned();
    let port = spawn_json_server(body).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick(500), vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn jsonpath_array_path_mismatch_fails() {
    install_expectations();
    let body = r#"{"status":"ok","items":[{"id":1,"name":"a"},{"id":2,"name":"z"},{"id":7,"name":"seven"}]}"#
        .to_owned();
    let port = spawn_json_server(body).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick(500), vec![target]).await;
    assert!(!report.all_ready());
}

#[tokio::test(flavor = "multi_thread")]
async fn jsonpath_filter_expression_works() {
    install_expectations();
    let body = r#"{"status":"ok","items":[{"id":1,"name":"a"},{"id":2,"name":"b"},{"id":7,"name":"other"}]}"#
        .to_owned();
    let port = spawn_json_server(body).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick(500), vec![target]).await;
    assert!(
        !report.all_ready(),
        "filter expression should reject when matched node value differs"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn jsonpath_missing_node_fails() {
    install_expectations();
    let body = r#"{"status":"ok"}"#.to_owned();
    let port = spawn_json_server(body).await;
    let target: Target = format!("http://127.0.0.1:{port}/").parse().unwrap();
    let report = run(quick(500), vec![target]).await;
    assert!(!report.all_ready());
}
