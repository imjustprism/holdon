#![forbid(unsafe_code)]
#![deny(unused_must_use, rust_2018_idioms)]

mod cli;
mod config;
mod output;
mod secret;

use std::process::{ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tokio::sync::mpsc;

use crate::cli::Args;
use crate::output::{Format, Printer};
use holdon::runner::RunnerConfig;
use holdon::{Runner, Target};

const EXIT_READY: u8 = 0;
const EXIT_MISUSE: u8 = 2;
const EXIT_EXEC_PERMISSION: u8 = 126;
const EXIT_EXEC_NOTFOUND: u8 = 127;
pub(crate) const DEFAULT_TIMEOUT_EXIT_CODE: u8 = 124;
const EXIT_SIGINT: u8 = 130;
const EXIT_SIGTERM: u8 = 143;

const SIG_NONE: u8 = 0;
const SIG_INT: u8 = 1;
const SIG_TERM: u8 = 2;

const INTERRUPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy)]
enum ExitStatus {
    Ready,
    Timeout(u8),
    Signal(u8),
    Misuse,
    ExecPermission,
    ExecNotFound,
}

impl ExitStatus {
    const fn code(self) -> u8 {
        match self {
            Self::Ready => EXIT_READY,
            Self::Timeout(c) | Self::Signal(c) => c,
            Self::Misuse => EXIT_MISUSE,
            Self::ExecPermission => EXIT_EXEC_PERMISSION,
            Self::ExecNotFound => EXIT_EXEC_NOTFOUND,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = Args::parse();

    if let Some(shell) = args.generate_completion {
        cli::print_completion(shell);
        return ExitCode::from(EXIT_READY);
    }
    if args.generate_manpage {
        if let Err(e) = cli::print_manpage() {
            eprintln!("holdon: writing manpage: {e}");
            return ExitCode::from(ExitStatus::Misuse.code());
        }
        return ExitCode::from(EXIT_READY);
    }

    match run(args).await {
        Ok(code) => ExitCode::from(code.code()),
        Err(e) => {
            eprintln!("holdon: {e:#}");
            ExitCode::from(ExitStatus::Misuse.code())
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn run(args: Args) -> Result<ExitStatus> {
    let config_data = config::load(args.config.as_deref())?;

    let mut raw_targets = collect_target_inputs(&args.targets)?;
    let cli_count = raw_targets.len();
    append_config_targets(&mut raw_targets, &config_data.targets)?;
    let mut names: Vec<Option<String>> = std::iter::repeat_n(None, raw_targets.len()).collect();
    let mut per_target_reverse: Vec<Option<bool>> =
        std::iter::repeat_n(None, raw_targets.len()).collect();
    let mut per_target_overrides: Vec<config::PerTargetOverride> =
        std::iter::repeat_n(config::PerTargetOverride::default(), raw_targets.len()).collect();
    let config_target_count = raw_targets.len() - cli_count;
    if config_target_count > 0 {
        for (i, n) in config_data
            .names
            .iter()
            .take(config_target_count)
            .cloned()
            .enumerate()
        {
            names[cli_count + i] = n;
        }
        for (i, d) in config_data
            .reverse_per_target
            .iter()
            .take(config_target_count)
            .copied()
            .enumerate()
        {
            per_target_reverse[cli_count + i] = d;
        }
        for (i, o) in config_data
            .overrides_per_target
            .iter()
            .take(config_target_count)
            .copied()
            .enumerate()
        {
            per_target_overrides[cli_count + i] = o;
        }
    }
    let resolved_targets: Vec<String> = raw_targets
        .iter()
        .map(|s| {
            let resolved = secret::resolve(s)
                .map(std::borrow::Cow::into_owned)
                .with_context(|| format!("resolving secrets in `{s}`"))?;
            if resolved.len() > MAX_TARGET_LEN {
                bail!(
                    "resolved target exceeds {MAX_TARGET_LEN} bytes (was `{s}` before substitution)"
                );
            }
            Ok(resolved)
        })
        .collect::<Result<_>>()?;
    let mut targets: Vec<Target> = resolved_targets
        .iter()
        .zip(raw_targets.iter())
        .map(|(resolved, raw)| {
            resolved
                .parse::<Target>()
                .with_context(|| format!("parsing `{raw}`"))
        })
        .collect::<Result<_>>()?;

    if let Some((lo, hi)) = args.expect_status {
        for t in &mut targets {
            if let Target::Http { expect, .. } = t {
                *expect = holdon::target::StatusRange::new(lo, hi);
            }
        }
    }

    #[cfg(feature = "http")]
    {
        let mut headers = holdon::checker::http::HeaderMap::with_capacity(args.headers.len());
        for h in &args.headers {
            headers.insert(h.name.clone(), h.value.clone());
        }
        if args.insecure {
            eprintln!("holdon: WARNING: TLS verification disabled (--insecure)");
        }
        let extra_ca_pem = match args.ca_cert.as_ref() {
            Some(path) => vec![
                std::fs::read(path)
                    .with_context(|| format!("reading --ca-cert from {}", path.display()))?,
            ],
            None => Vec::new(),
        };
        let client_identity_pem = match (args.client_cert.as_ref(), args.client_key.as_ref()) {
            (Some(cert), Some(key)) => {
                let mut bundle = std::fs::read(cert)
                    .with_context(|| format!("reading --client-cert from {}", cert.display()))?;
                let key_bytes = std::fs::read(key)
                    .with_context(|| format!("reading --client-key from {}", key.display()))?;
                if !bundle.ends_with(b"\n") {
                    bundle.push(b'\n');
                }
                bundle.extend_from_slice(&key_bytes);
                Some(bundle)
            }
            _ => None,
        };
        let header_expectations = args
            .expect_headers
            .iter()
            .map(|h| (h.name.clone(), h.pattern.clone()))
            .collect();
        holdon::checker::http::set_global(holdon::checker::http::HttpConfig {
            headers,
            method: args.method.into(),
            insecure: args.insecure,
            follow_redirects: !args.no_follow_redirects,
            body_substring: args.expect_body.clone(),
            body_regex: args.expect_body_regex.clone(),
            body_json_match: args.expect_json.clone(),
            extra_ca_pem,
            min_tls: args.tls_min.into(),
            body: args.data.as_ref().map(|s| s.as_bytes().to_vec()),
            client_identity_pem,
            header_expectations,
            http2_prior_knowledge: args.http2_prior_knowledge,
            max_rtt: args.max_rtt,
            max_redirects: args.max_redirects,
        });
    }

    if targets.is_empty() {
        bail!("no targets given");
    }

    let merge_dur = |cli: Duration, cli_default: Duration, conf: Option<Duration>| -> Duration {
        if cli == cli_default {
            conf.unwrap_or(cli)
        } else {
            cli
        }
    };
    let cfg = RunnerConfig::default()
        .timeout(merge_dur(
            args.timeout,
            RunnerConfig::DEFAULT_OVERALL_TIMEOUT,
            config_data.timeout,
        ))
        .interval(merge_dur(
            args.interval,
            RunnerConfig::DEFAULT_INITIAL_INTERVAL,
            config_data.interval,
        ))
        .max_interval(merge_dur(
            args.max_interval,
            RunnerConfig::DEFAULT_MAX_INTERVAL,
            config_data.max_interval,
        ))
        .initial_delay(merge_dur(
            args.initial_delay,
            RunnerConfig::DEFAULT_INITIAL_DELAY,
            config_data.initial_delay,
        ))
        .attempt_timeout(merge_dur(
            args.attempt_timeout,
            RunnerConfig::DEFAULT_ATTEMPT_TIMEOUT,
            config_data.attempt_timeout,
        ))
        .reverse(args.reverse || config_data.reverse.unwrap_or(false))
        .once(args.once || config_data.once.unwrap_or(false))
        .sequential(args.sequential || config_data.sequential.unwrap_or(false))
        .success_threshold(
            if args.success_threshold == RunnerConfig::DEFAULT_SUCCESS_THRESHOLD {
                config_data
                    .success_threshold
                    .unwrap_or(args.success_threshold)
            } else {
                args.success_threshold
            },
        )
        .jitter(!args.no_jitter && config_data.jitter.unwrap_or(true));

    let global_reverse = args.reverse || config_data.reverse.unwrap_or(false);
    let has_per_target = per_target_reverse.iter().any(Option::is_some);
    let cfg = if has_per_target {
        let directions: Vec<holdon::Direction> = per_target_reverse
            .iter()
            .map(|d| {
                let reversed = d.unwrap_or(global_reverse);
                if reversed {
                    holdon::Direction::Reverse
                } else {
                    holdon::Direction::Wait
                }
            })
            .collect();
        cfg.directions(Some(directions))
    } else {
        cfg
    };

    let has_any_override = per_target_overrides
        .iter()
        .any(config::PerTargetOverride::is_some);
    let cfg = if has_any_override {
        let overrides: Vec<holdon::runner::TargetOverrides> = per_target_overrides
            .iter()
            .copied()
            .map(Into::into)
            .collect();
        cfg.overrides(Some(overrides))
    } else {
        cfg
    };

    if args.validate {
        print_plan(&targets, &names, &cfg).context("writing --validate plan")?;
        return Ok(ExitStatus::Ready);
    }

    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let color =
        !args.no_color && !no_color_env && std::io::IsTerminal::is_terminal(&std::io::stderr());
    let format = if args.quiet {
        Format::Quiet
    } else {
        args.output.into()
    };
    let mut printer = Printer::new(format, color);
    #[cfg(feature = "json-output")]
    let mut log_file = open_log_file(args.log_file.as_deref())?;
    #[cfg(not(feature = "json-output"))]
    if args.log_file.is_some() {
        eprintln!("holdon: --log-file requires the `json-output` feature, ignoring");
    }

    let exec_slice: Option<&[String]> = if args.exec.is_empty() {
        None
    } else {
        Some(&args.exec)
    };
    #[cfg(feature = "json-output")]
    {
        if printer.is_json() || log_file.is_some() {
            let v = output::json::start_value(&targets, &names);
            if printer.is_json() {
                output::json::emit_value(&v);
            } else {
                printer.banner(&targets, &names, exec_slice);
            }
            write_log_line(&mut log_file, &v);
        } else {
            printer.banner(&targets, &names, exec_slice);
        }
    }
    #[cfg(not(feature = "json-output"))]
    printer.banner(&targets, &names, exec_slice);

    install_panic_hook();
    init_tracing(args.verbose);
    let interrupted = install_signal_handlers();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg_for_runner = cfg.clone();
    let runner = Runner::new(cfg_for_runner);
    let run_handle = tokio::spawn(runner.run(targets, Some(tx)));
    let mut ticker = tokio::time::interval(printer.tick_interval());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut signal_fired = SIG_NONE;
    loop {
        tokio::select! {
            biased;
            ev = rx.recv() => match ev {
                Some(ev) => {
                    #[cfg(feature = "json-output")]
                    {
                        if printer.is_json() || log_file.is_some() {
                            if let Some(v) = output::json::event_value(&ev) {
                                if printer.is_json() {
                                    output::json::emit_value(&v);
                                } else {
                                    printer.handle(&ev);
                                }
                                write_log_line(&mut log_file, &v);
                            } else if !printer.is_json() {
                                printer.handle(&ev);
                            }
                        } else {
                            printer.handle(&ev);
                        }
                    }
                    #[cfg(not(feature = "json-output"))]
                    printer.handle(&ev);
                }
                None => break,
            },
            _ = ticker.tick() => printer.tick(),
            sig = wait_interrupt(&interrupted) => {
                signal_fired = sig;
                run_handle.abort();
                break;
            }
        }
    }

    let report = match run_handle.await {
        Ok(r) => r,
        Err(je) if je.is_cancelled() => {
            eprintln!("holdon: interrupted");
            #[cfg(feature = "json-output")]
            if log_file.is_some() {
                let v = serde_json::json!({
                    "v": output::json::VERSION,
                    "ts_unix_ms": output::json::now_unix_ms(),
                    "event": "end",
                    "interrupted": true,
                });
                write_log_line(&mut log_file, &v);
            }
            return Ok(ExitStatus::Signal(signal_exit_code(signal_fired)));
        }
        Err(je) => {
            return Err(anyhow::anyhow!("runner task panicked: {je}"));
        }
    };

    #[cfg(feature = "json-output")]
    {
        if printer.is_json() || log_file.is_some() {
            let values = output::json::summary_values(&report);
            if printer.is_json() {
                for v in &values {
                    output::json::emit_value(v);
                }
            } else {
                printer.summary(&report, exec_slice);
            }
            for v in &values {
                write_log_line(&mut log_file, v);
            }
        } else {
            printer.summary(&report, exec_slice);
        }
    }
    #[cfg(not(feature = "json-output"))]
    printer.summary(&report, exec_slice);

    let at_least = args.at_least.or(config_data.at_least);
    let ready = if let Some(n) = at_least {
        report.results.iter().filter(|r| r.satisfied).count() >= n.max(1)
    } else {
        report.all_ready()
    };

    #[cfg(feature = "http")]
    {
        let url = if ready {
            args.on_ready.as_deref()
        } else {
            args.on_fail.as_deref()
        };
        if let Some(url) = url {
            fire_webhook(url, ready, &report, args.webhook_timeout).await;
        }
    }

    if args.watch && !ready {
        eprintln!("holdon: --watch skipped: not all targets became ready");
    } else if args.watch {
        if args.watch_interval.is_zero() {
            eprintln!("holdon: --watch-interval must be greater than zero");
            return Ok(ExitStatus::Misuse);
        }
        if !args.exec.is_empty() {
            eprintln!("holdon: warning: --watch is active, --exec command will not run");
        }
        let targets_for_watch: Vec<Target> =
            report.results.iter().map(|r| r.target.clone()).collect();
        let initial_ready: Vec<bool> = report.results.iter().map(|r| r.satisfied).collect();
        let watch_reverse: Vec<bool> = per_target_reverse
            .iter()
            .map(|d| d.unwrap_or(global_reverse))
            .collect();
        let watch_attempt_timeouts: Vec<Duration> = per_target_overrides
            .iter()
            .map(|o| {
                o.attempt_timeout
                    .unwrap_or(cfg.attempt_timeout)
                    .max(RunnerConfig::MIN_INTERVAL)
            })
            .collect();
        watch_loop(
            targets_for_watch,
            initial_ready,
            watch_attempt_timeouts,
            args.watch_interval,
            watch_reverse,
            #[cfg(feature = "http")]
            args.on_transition.clone(),
            #[cfg(feature = "http")]
            args.webhook_timeout,
            &interrupted,
        )
        .await;
        let sig = interrupted.load(Ordering::SeqCst);
        return Ok(if sig == SIG_NONE {
            ExitStatus::Misuse
        } else {
            ExitStatus::Signal(signal_exit_code(sig))
        });
    }

    let should_exec = !args.exec.is_empty() && (ready || !args.strict);

    if let (true, Some((program, rest))) = (should_exec, args.exec.split_first()) {
        let program_path = resolve_program(program);
        let spawned = tokio::process::Command::new(&program_path)
            .args(rest)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                eprintln!("holdon: exec `{program}`: {e:#}");
                return Ok(match e.kind() {
                    std::io::ErrorKind::PermissionDenied => ExitStatus::ExecPermission,
                    std::io::ErrorKind::NotFound => ExitStatus::ExecNotFound,
                    _ => ExitStatus::Misuse,
                });
            }
        };
        let status = tokio::select! {
            biased;
            r = child.wait() => r,
            sig = wait_interrupt(&interrupted) => {
                forward_signal_to_child(&mut child, sig).await;
                return Ok(ExitStatus::Signal(signal_exit_code(sig)));
            }
        };
        match status {
            Ok(s) if !s.success() => {
                let code = s.code().unwrap_or(1);
                return Ok(match u8::try_from(code).ok() {
                    Some(c) if c == EXIT_EXEC_PERMISSION => ExitStatus::ExecPermission,
                    Some(c) if c == EXIT_EXEC_NOTFOUND => ExitStatus::ExecNotFound,
                    _ => ExitStatus::Timeout(args.timeout_exit_code),
                });
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("holdon: wait `{program}`: {e:#}");
                return Ok(ExitStatus::Misuse);
            }
        }
    }

    Ok(if ready {
        ExitStatus::Ready
    } else {
        ExitStatus::Timeout(args.timeout_exit_code)
    })
}

pub(crate) const MAX_TARGETS: usize = 10_000;
pub(crate) const MAX_TARGET_LEN: usize = 2048;
const UTF8_BOM: &str = "\u{feff}";

fn push_validated(out: &mut Vec<String>, s: String) -> Result<()> {
    if s.len() > MAX_TARGET_LEN {
        bail!("target string exceeds {MAX_TARGET_LEN} bytes");
    }
    if out.len() >= MAX_TARGETS {
        bail!("too many targets (max {MAX_TARGETS})");
    }
    out.push(s);
    Ok(())
}

fn collect_target_inputs(args: &[String]) -> Result<Vec<String>> {
    use std::io::BufRead;
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        if a == "-" {
            let stdin = std::io::stdin();
            let mut first = true;
            for line in stdin.lock().lines() {
                let mut line = line.context("reading stdin")?;
                if first {
                    if let Some(rest) = line.strip_prefix(UTF8_BOM) {
                        line = rest.to_owned();
                    }
                    first = false;
                }
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    push_validated(&mut out, trimmed.to_owned())?;
                }
            }
        } else {
            push_validated(&mut out, a.clone())?;
        }
    }
    Ok(out)
}

fn append_config_targets(out: &mut Vec<String>, config_targets: &[String]) -> Result<()> {
    for t in config_targets {
        push_validated(out, t.clone())?;
    }
    Ok(())
}

#[cfg(windows)]
const SAFE_EXTS: &[&str] = &[".com", ".exe"];

#[cfg(windows)]
fn resolve_program(program: &str) -> std::path::PathBuf {
    use std::path::Path;
    let p = Path::new(program);
    if p.is_absolute() || program.contains('/') || program.contains('\\') {
        return p.to_path_buf();
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        for ext in SAFE_EXTS {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return candidate;
            }
        }
        let bare = dir.join(program);
        if bare.is_file() {
            return bare;
        }
    }
    p.to_path_buf()
}

#[cfg(not(windows))]
fn resolve_program(program: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(program)
}

fn init_tracing(verbosity: u8) {
    use tracing::level_filters::LevelFilter;
    let level = std::env::var("HOLDON_LOG")
        .ok()
        .and_then(|s| s.parse::<LevelFilter>().ok())
        .unwrap_or(match verbosity {
            0 => LevelFilter::WARN,
            1 => LevelFilter::INFO,
            2 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        });
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .try_init();
}

fn install_signal_handlers() -> Arc<AtomicU8> {
    let flag = Arc::new(AtomicU8::new(SIG_NONE));
    let flag_c = Arc::clone(&flag);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = flag_c.compare_exchange(SIG_NONE, SIG_INT, Ordering::SeqCst, Ordering::SeqCst);
    });

    #[cfg(unix)]
    {
        let flag_t = Arc::clone(&flag);
        tokio::spawn(async move {
            if let Ok(mut term) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                term.recv().await;
                let _ =
                    flag_t.compare_exchange(SIG_NONE, SIG_TERM, Ordering::SeqCst, Ordering::SeqCst);
            }
        });
    }

    flag
}

async fn wait_interrupt(flag: &AtomicU8) -> u8 {
    loop {
        let v = flag.load(Ordering::Relaxed);
        if v != SIG_NONE {
            return v;
        }
        tokio::time::sleep(INTERRUPT_POLL_INTERVAL).await;
    }
}

async fn forward_signal_to_child(child: &mut tokio::process::Child, _sig: u8) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
}

#[cfg(feature = "json-output")]
fn open_log_file(
    path: Option<&std::path::Path>,
) -> Result<Option<std::io::BufWriter<std::fs::File>>> {
    let Some(p) = path else {
        return Ok(None);
    };
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
        .with_context(|| format!("opening --log-file {}", p.display()))?;
    Ok(Some(std::io::BufWriter::new(f)))
}

#[cfg(feature = "json-output")]
fn write_log_line(sink: &mut Option<std::io::BufWriter<std::fs::File>>, value: &serde_json::Value) {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    let Some(w) = sink.as_mut() else {
        return;
    };
    let Ok(s) = serde_json::to_string(value) else {
        return;
    };
    let result = writeln!(w, "{s}").and_then(|()| w.flush());
    if let Err(e) = result {
        if !WARNED.swap(true, Ordering::Relaxed) {
            eprintln!("holdon: --log-file write failed: {e} (further errors suppressed)");
        }
    }
}

fn print_plan(
    targets: &[Target],
    names: &[Option<String>],
    cfg: &RunnerConfig,
) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    writeln!(out, "holdon validate plan:")?;
    writeln!(out, "  targets ({}):", targets.len())?;
    for (i, t) in targets.iter().enumerate() {
        let label = names
            .get(i)
            .and_then(Option::as_ref)
            .map(|s| format!(" [{s}]"))
            .unwrap_or_default();
        writeln!(out, "    {i}{label}: {t}")?;
    }
    writeln!(out, "  schedule: {:?}", cfg.schedule)?;
    writeln!(out, "  direction: {:?}", cfg.direction)?;
    writeln!(out, "  overall_timeout: {:?}", cfg.overall_timeout)?;
    writeln!(out, "  initial_interval: {:?}", cfg.initial_interval)?;
    writeln!(out, "  max_interval: {:?}", cfg.max_interval)?;
    writeln!(out, "  initial_delay: {:?}", cfg.initial_delay)?;
    writeln!(out, "  attempt_timeout: {:?}", cfg.attempt_timeout)?;
    writeln!(out, "  success_threshold: {}", cfg.success_threshold)?;
    writeln!(out, "  jitter: {}", cfg.jitter)?;
    writeln!(out, "  once: {}", cfg.once)?;
    if let Some(per) = &cfg.directions {
        writeln!(out, "  per_target_direction: {per:?}")?;
    }
    if let Some(per) = &cfg.overrides {
        writeln!(out, "  per_target_overrides: {per:?}")?;
    }
    writeln!(out, "validation ok")?;
    out.flush()
}

const fn signal_exit_code(sig: u8) -> u8 {
    match sig {
        SIG_TERM => EXIT_SIGTERM,
        _ => EXIT_SIGINT,
    }
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(std::io::stderr(), crossterm::cursor::Show);
        prev(info);
    }));
}

#[cfg(feature = "http")]
async fn fire_webhook(url: &str, ready: bool, report: &holdon::Report, timeout: Duration) {
    use serde_json::json;
    let body = json!({
        "event": if ready { "ready" } else { "fail" },
        "elapsed_ms": u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX),
        "targets": report.results.iter().map(|r| json!({
            "idx": r.idx,
            "target": r.target.to_string(),
            "satisfied": r.satisfied,
            "attempts": r.attempts,
        })).collect::<Vec<_>>(),
    });
    send_json(url, &body.to_string(), timeout).await;
}

#[cfg(feature = "http")]
fn webhook_client() -> Option<&'static reqwest::Client> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(concat!("holdon/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| eprintln!("holdon: webhook client build failed: {e}"))
                .ok()
        })
        .as_ref()
}

#[cfg(feature = "http")]
async fn send_json(url: &str, body: &str, timeout: Duration) {
    let Some(client) = webhook_client() else {
        return;
    };
    let request = client
        .post(url)
        .timeout(timeout)
        .header("content-type", "application/json")
        .body(body.to_owned());
    match request.send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                eprintln!("holdon: webhook {url} returned {status}");
            }
        }
        Err(e) => {
            eprintln!("holdon: webhook {url} failed: {e}");
        }
    }
}

#[cfg(feature = "http")]
async fn drain_webhooks(set: &mut tokio::task::JoinSet<()>, per_task_timeout: Duration) {
    let budget = per_task_timeout.saturating_add(Duration::from_millis(500));
    let _ = tokio::time::timeout(budget, async { while set.join_next().await.is_some() {} }).await;
    set.abort_all();
}

#[cfg(feature = "http")]
async fn fire_transition_webhook(
    url: &str,
    idx: usize,
    target: &Target,
    ready: bool,
    timeout: Duration,
) {
    use serde_json::json;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let body = json!({
        "event": "transition",
        "idx": idx,
        "target": target.to_string(),
        "transition": if ready { "fail->ready" } else { "ready->fail" },
        "ready": ready,
        "at": now,
    });
    send_json(url, &body.to_string(), timeout).await;
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn watch_loop(
    targets: Vec<Target>,
    initial_ready: Vec<bool>,
    attempt_timeouts: Vec<Duration>,
    interval: Duration,
    reverse_per_target: Vec<bool>,
    #[cfg(feature = "http")] on_transition: Option<String>,
    #[cfg(feature = "http")] webhook_timeout: Duration,
    interrupted: &Arc<AtomicU8>,
) {
    use std::io::Write as _;
    let mut last_ready: Vec<bool> = initial_ready;
    if last_ready.len() != targets.len() {
        last_ready.resize(targets.len(), true);
    }
    if interval.is_zero() {
        let _ = writeln!(
            std::io::stderr(),
            "holdon: --watch-interval must be greater than zero"
        );
        return;
    }
    #[cfg(feature = "http")]
    let mut webhook_tasks: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    let _ = writeln!(
        std::io::stderr(),
        "holdon: watch mode, interval={}s (Ctrl-C to exit)",
        interval.as_secs_f64()
    );
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;
    loop {
        tokio::select! {
            biased;
            _ = wait_interrupt(interrupted) => {
                let _ = writeln!(std::io::stderr(), "holdon: watch exiting on signal");
                #[cfg(feature = "http")]
                drain_webhooks(&mut webhook_tasks, webhook_timeout).await;
                return;
            }
            _ = ticker.tick() => {
                let mut set = tokio::task::JoinSet::new();
                for (idx, target) in targets.iter().enumerate() {
                    let target = target.clone();
                    let reverse = reverse_per_target.get(idx).copied().unwrap_or(false);
                    let mut probe_ctx = holdon::checker::AttemptCtx::default();
                    if let Some(t) = attempt_timeouts.get(idx).copied() {
                        probe_ctx.attempt_timeout = t;
                    }
                    set.spawn(async move {
                        let outcome = target.probe(probe_ctx).await;
                        let ready = if reverse {
                            !outcome.is_ready()
                        } else {
                            outcome.is_ready()
                        };
                        (idx, ready, target)
                    });
                }
                let mut aborted = false;
                loop {
                    tokio::select! {
                        biased;
                        _ = wait_interrupt(interrupted) => {
                            set.abort_all();
                            aborted = true;
                            break;
                        }
                        joined = set.join_next() => match joined {
                            Some(Ok((idx, ready, target))) => {
                                if let Some(prev) = last_ready.get_mut(idx) {
                                    if ready != *prev {
                                        let arrow = if ready { "fail -> ready" } else { "ready -> fail" };
                                        let _ = writeln!(
                                            std::io::stderr(),
                                            "holdon: [{idx}] {target}: {arrow}"
                                        );
                                        *prev = ready;
                                        #[cfg(feature = "http")]
                                        if let Some(hook) = on_transition.clone() {
                                            let target_clone = target.clone();
                                            let timeout_clone = webhook_timeout;
                                            webhook_tasks.spawn(async move {
                                                fire_transition_webhook(
                                                    &hook,
                                                    idx,
                                                    &target_clone,
                                                    ready,
                                                    timeout_clone,
                                                )
                                                .await;
                                            });
                                        }
                                    }
                                }
                            }
                            Some(Err(_)) => {}
                            None => break,
                        }
                    }
                }
                if aborted {
                    let _ = writeln!(std::io::stderr(), "holdon: watch exiting on signal");
                    #[cfg(feature = "http")]
                    drain_webhooks(&mut webhook_tasks, webhook_timeout).await;
                    return;
                }
            }
        }
    }
}
