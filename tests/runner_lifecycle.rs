#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::drop_non_drop,
    clippy::single_char_pattern
)]

mod common;

use std::time::Duration;

use common::{bind_ephemeral, free_port};
use holdon::runner::{Event, RunnerConfig};
use holdon::{Runner, Target};
use tokio::sync::mpsc;
use tokio::time::Instant;

#[tokio::test(flavor = "multi_thread")]
async fn parallel_finishes_in_under_max_attempt_time() {
    let p1 = free_port().await;
    let p2 = free_port().await;
    let p3 = free_port().await;
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(800))
        .attempt_timeout(Duration::from_millis(300))
        .once(true);
    let targets: Vec<Target> = [p1, p2, p3]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let start = Instant::now();
    let report = Runner::new(cfg).run(targets, None).await;
    let elapsed = start.elapsed();
    assert!(!report.all_ready());
    assert!(
        elapsed < Duration::from_millis(900),
        "parallel run took {elapsed:?}, should be ~300ms"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sequential_runs_in_order() {
    let (_l1, p1) = bind_ephemeral().await;
    let (_l2, p2) = bind_ephemeral().await;
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(2))
        .sequential(true)
        .once(true);
    let targets: Vec<Target> = [p1, p2]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let report = Runner::new(cfg).run(targets, None).await;
    assert!(report.all_ready());
    assert_eq!(report.results.len(), 2);
    assert_eq!(report.results[0].idx, 0);
    assert_eq!(report.results[1].idx, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn report_results_sorted_by_input_idx() {
    let (_l1, slow_port) = bind_ephemeral().await;
    let (_l2, fast_port) = bind_ephemeral().await;
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(2))
        .once(true);
    let targets: Vec<Target> = [slow_port, fast_port]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let report = Runner::new(cfg).run(targets, None).await;
    assert_eq!(report.results[0].idx, 0);
    assert_eq!(report.results[1].idx, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_run_future_cancels_in_flight_probes() {
    let port = free_port().await;
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(60))
        .attempt_timeout(Duration::from_secs(30))
        .interval(Duration::from_millis(10));
    let target: Target = format!("127.0.0.1:{port}").parse().unwrap();
    let runner = Runner::new(cfg);
    let fut = runner.run(vec![target], None);
    tokio::pin!(fut);
    let start = Instant::now();
    let res = tokio::time::timeout(Duration::from_millis(200), &mut fut).await;
    assert!(res.is_err(), "future completed too fast");
    drop(fut);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "drop took {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_targets_yields_not_ready_report() {
    let cfg = RunnerConfig::default().timeout(Duration::from_millis(50));
    let report = Runner::new(cfg).run(vec![], None).await;
    assert!(!report.all_ready(), "empty targets must NOT vacuously pass");
    assert!(report.assert_all_ready().is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn events_emitted_for_each_attempt_and_finish() {
    let port = free_port().await;
    let target: Target = format!("127.0.0.1:{port}").parse().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(800))
        .attempt_timeout(Duration::from_millis(150))
        .interval(Duration::from_millis(50))
        .max_interval(Duration::from_millis(100));
    let handle = tokio::spawn(Runner::new(cfg).run(vec![target], Some(tx)));

    let mut attempts = 0u32;
    let mut finished = 0u32;
    while let Some(ev) = rx.recv().await {
        match ev {
            Event::Attempt { ready, .. } => {
                attempts += 1;
                assert!(!ready, "closed port should never report ready");
            }
            Event::Finished { satisfied, .. } => {
                finished += 1;
                assert!(!satisfied);
            }
            _ => {}
        }
    }
    let _ = handle.await.unwrap();
    assert!(attempts >= 2, "expected multiple attempts, got {attempts}");
    assert_eq!(finished, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn deadline_overall_timeout_respected() {
    let port = free_port().await;
    let target: Target = format!("127.0.0.1:{port}").parse().unwrap();
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(500))
        .attempt_timeout(Duration::from_millis(200))
        .interval(Duration::from_millis(50));
    let start = Instant::now();
    let report = Runner::new(cfg).run(vec![target], None).await;
    let elapsed = start.elapsed();
    assert!(!report.all_ready());
    assert!(elapsed < Duration::from_millis(900), "elapsed={elapsed:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn prereqs_gate_dependent_target() {
    let (l_a, p_a) = bind_ephemeral().await;
    let _open_a = Box::leak(Box::new(l_a));
    let (l_b, p_b) = bind_ephemeral().await;
    let _open_b = Box::leak(Box::new(l_b));
    let prereqs = vec![Vec::new(), vec![0usize]];
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(3))
        .attempt_timeout(Duration::from_millis(200))
        .once(true)
        .prereqs(Some(prereqs));
    let targets: Vec<Target> = [p_a, p_b]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let report = Runner::new(cfg).run(targets, None).await;
    assert!(report.all_ready(), "both targets should be satisfied");
    assert_eq!(report.results.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn prereqs_sequential_reorders_by_dependency() {
    let (l_a, p_a) = bind_ephemeral().await;
    let _open_a = Box::leak(Box::new(l_a));
    let (l_b, p_b) = bind_ephemeral().await;
    let _open_b = Box::leak(Box::new(l_b));
    let prereqs = vec![vec![1usize], Vec::new()];
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(3))
        .attempt_timeout(Duration::from_millis(200))
        .sequential(true)
        .once(true)
        .prereqs(Some(prereqs));
    let targets: Vec<Target> = [p_a, p_b]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let start = Instant::now();
    let report = Runner::new(cfg).run(targets, None).await;
    let elapsed = start.elapsed();
    assert!(
        report.all_ready(),
        "sequential mode with backwards dependency should still complete"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "sequential mode should not deadlock, elapsed={elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn prereqs_deadline_marks_dependent_not_ready() {
    let port_closed = free_port().await;
    let (l_b, p_b) = bind_ephemeral().await;
    let _open_b = Box::leak(Box::new(l_b));
    let prereqs = vec![Vec::new(), vec![0usize]];
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(400))
        .attempt_timeout(Duration::from_millis(100))
        .interval(Duration::from_millis(50))
        .prereqs(Some(prereqs));
    let targets: Vec<Target> = [port_closed, p_b]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let report = Runner::new(cfg).run(targets, None).await;
    assert!(!report.all_ready());
    let dep = report.results.iter().find(|r| r.idx == 1).unwrap();
    assert!(!dep.satisfied);
    assert_eq!(dep.attempts, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn prereqs_fail_short_circuits_dependent() {
    let port_closed = free_port().await;
    let (l_b, p_b) = bind_ephemeral().await;
    let _open_b = Box::leak(Box::new(l_b));
    let prereqs = vec![Vec::new(), vec![0usize]];
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(800))
        .attempt_timeout(Duration::from_millis(100))
        .interval(Duration::from_millis(50))
        .max_attempts(Some(3))
        .prereqs(Some(prereqs));
    let targets: Vec<Target> = [port_closed, p_b]
        .iter()
        .map(|p| format!("127.0.0.1:{p}").parse().unwrap())
        .collect();
    let report = Runner::new(cfg).run(targets, None).await;
    assert!(!report.all_ready());
    let dep = report.results.iter().find(|r| r.idx == 1).unwrap();
    assert!(!dep.satisfied, "dependent target should not be satisfied");
    assert_eq!(dep.attempts, 0, "dependent target should never have probed");
}

#[tokio::test(flavor = "multi_thread")]
async fn max_attempts_caps_retry_loop() {
    let port = free_port().await;
    let target: Target = format!("127.0.0.1:{port}").parse().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(30))
        .attempt_timeout(Duration::from_millis(100))
        .interval(Duration::from_millis(5))
        .jitter(false)
        .max_attempts(Some(3));
    let handle = tokio::spawn(Runner::new(cfg).run(vec![target], Some(tx)));

    let mut attempts = 0u32;
    while let Some(ev) = rx.recv().await {
        if matches!(ev, Event::Attempt { .. }) {
            attempts += 1;
        }
    }
    let report = handle.await.unwrap();
    assert!(!report.all_ready());
    assert_eq!(attempts, 3, "max_attempts should cap retries at 3");
}

#[tokio::test(flavor = "multi_thread")]
async fn once_makes_exactly_one_attempt() {
    let port = free_port().await;
    let target: Target = format!("127.0.0.1:{port}").parse().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(5))
        .attempt_timeout(Duration::from_millis(200))
        .once(true);
    let handle = tokio::spawn(Runner::new(cfg).run(vec![target], Some(tx)));

    let mut attempts = 0u32;
    while let Some(ev) = rx.recv().await {
        if matches!(ev, Event::Attempt { .. }) {
            attempts += 1;
        }
    }
    let _ = handle.await.unwrap();
    assert_eq!(attempts, 1, "once should make exactly one attempt");
}

#[tokio::test(flavor = "multi_thread")]
async fn per_target_attempt_timeout_overrides_global() {
    let (slow_listener, slow_port) = bind_ephemeral().await;
    let _slow = tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = slow_listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 1];
                let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                tokio::time::sleep(Duration::from_secs(60)).await;
            });
        }
    });
    let target_slow: Target = format!("http://127.0.0.1:{slow_port}/").parse().unwrap();

    let (l_open, port_open) = bind_ephemeral().await;
    let _open = Box::leak(Box::new(l_open));
    let target_open: Target = format!("127.0.0.1:{port_open}").parse().unwrap();

    let mut ov_short = holdon::TargetOverrides::default();
    ov_short.attempt_timeout = Some(Duration::from_millis(150));
    let overrides = vec![ov_short, holdon::TargetOverrides::default()];
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(3))
        .attempt_timeout(Duration::from_secs(10))
        .once(true)
        .overrides(Some(overrides));
    let start = Instant::now();
    let report = Runner::new(cfg)
        .run(vec![target_slow, target_open], None)
        .await;
    let elapsed = start.elapsed();
    assert!(!report.results[0].satisfied);
    assert!(report.results[1].satisfied);
    assert!(
        elapsed < Duration::from_secs(2),
        "per-target attempt_timeout not honoured, elapsed={elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn per_target_success_threshold_overrides_global() {
    let (_l, p) = bind_ephemeral().await;
    let target: Target = format!("127.0.0.1:{p}").parse().unwrap();
    let mut ov = holdon::TargetOverrides::default();
    ov.success_threshold = Some(3);
    ov.interval = Some(Duration::from_millis(5));
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(3))
        .attempt_timeout(Duration::from_millis(200))
        .success_threshold(1)
        .jitter(false)
        .overrides(Some(vec![ov]));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = tokio::spawn(Runner::new(cfg).run(vec![target], Some(tx)));
    let mut attempts = 0u32;
    while let Some(ev) = rx.recv().await {
        if matches!(ev, Event::Attempt { .. }) {
            attempts += 1;
        }
    }
    let report = handle.await.unwrap();
    assert!(report.all_ready());
    assert!(
        attempts >= 3,
        "expected >=3 attempts to satisfy threshold, got {attempts}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn assert_all_ready_returns_not_ready_error() {
    let port = free_port().await;
    let target: Target = format!("127.0.0.1:{port}").parse().unwrap();
    let cfg = RunnerConfig::default()
        .timeout(Duration::from_millis(500))
        .attempt_timeout(Duration::from_millis(200))
        .once(true);
    let report = Runner::new(cfg).run(vec![target], None).await;
    let err = report.assert_all_ready().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("1") && msg.contains("failed"), "msg={msg}");
}
