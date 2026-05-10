//! Wait for a Postgres and Redis instance from a Rust program.
//!
//! ```no_run
//! cargo run --example wait_for_services --features full
//! ```

use std::time::Duration;

use holdon::runner::RunnerConfig;
use holdon::{Runner, Target};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let targets = vec![
        "postgres-host:5432".parse::<Target>()?,
        "redis-host:6379".parse::<Target>()?,
    ];

    let cfg = RunnerConfig::default()
        .timeout(Duration::from_secs(30))
        .interval(Duration::from_millis(200));

    let report = Runner::new(cfg).run(targets, None).await;
    report.assert_all_ready()?;

    println!("all services ready in {:?}", report.elapsed);
    Ok(())
}
