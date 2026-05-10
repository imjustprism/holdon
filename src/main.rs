mod cli;
mod output;

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
    let raw_targets = collect_target_inputs(&args.targets)?;
    let mut targets: Vec<Target> = raw_targets
        .iter()
        .map(|s| {
            s.parse::<Target>()
                .with_context(|| format!("parsing `{s}`"))
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
        holdon::checker::http::set_global(holdon::checker::http::HttpConfig {
            headers,
            method: args.method.into(),
            insecure: args.insecure,
        });
    }

    if targets.is_empty() {
        bail!("no targets given");
    }

    let cfg = RunnerConfig::default()
        .timeout(args.timeout)
        .interval(args.interval)
        .max_interval(args.max_interval)
        .initial_delay(args.initial_delay)
        .attempt_timeout(args.attempt_timeout)
        .reverse(args.reverse)
        .once(args.once)
        .sequential(args.sequential)
        .success_threshold(args.success_threshold)
        .jitter(!args.no_jitter);

    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let color =
        !args.no_color && !no_color_env && std::io::IsTerminal::is_terminal(&std::io::stderr());
    let format = if args.quiet {
        Format::Quiet
    } else {
        args.output.into()
    };
    let mut printer = Printer::new(format, color);

    let exec_slice: Option<&[String]> = if args.exec.is_empty() {
        None
    } else {
        Some(&args.exec)
    };
    printer.banner(&targets, exec_slice);

    install_panic_hook();
    init_tracing(args.verbose);
    let interrupted = install_signal_handlers();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let runner = Runner::new(cfg);
    let run_handle = tokio::spawn(runner.run(targets, Some(tx)));
    let mut ticker = tokio::time::interval(printer.tick_interval());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut signal_fired = SIG_NONE;
    loop {
        tokio::select! {
            biased;
            ev = rx.recv() => match ev {
                Some(ev) => printer.handle(&ev),
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
            return Ok(ExitStatus::Signal(signal_exit_code(signal_fired)));
        }
        Err(je) => {
            return Err(anyhow::anyhow!("runner task panicked: {je}"));
        }
    };

    printer.summary(&report, exec_slice);

    let ready = if let Some(n) = args.at_least {
        report.results.iter().filter(|r| r.satisfied).count() >= n.max(1)
    } else {
        report.all_ready()
    };
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

fn collect_target_inputs(args: &[String]) -> Result<Vec<String>> {
    use std::io::BufRead;
    let mut out = Vec::with_capacity(args.len());
    let mut push = |s: String| -> Result<()> {
        if s.len() > MAX_TARGET_LEN {
            bail!("target string exceeds {MAX_TARGET_LEN} bytes");
        }
        if out.len() >= MAX_TARGETS {
            bail!("too many targets (max {MAX_TARGETS})");
        }
        out.push(s);
        Ok(())
    };
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
                    push(trimmed.to_owned())?;
                }
            }
        } else {
            push(a.clone())?;
        }
    }
    Ok(out)
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
