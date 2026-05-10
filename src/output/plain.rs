use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::{cursor, execute, queue, terminal};

use super::style::Style;
use super::theme;
use super::theme::Color;
use holdon::Target;
use holdon::diagnostic::{Stage, StageResult};
use holdon::runner::{Event, Report, TargetReport};
use holdon::util::fmt_dur;

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Plain {
    style: Style,
    quiet: bool,
    states: Vec<Slot>,
    started: Instant,
    last_lines: u16,
    live: bool,
    cursor_hidden: bool,
    spinner_frame: usize,
}

const SPARK_HISTORY: usize = 8;
const DEFAULT_TERMINAL_WIDTH: usize = 120;
const MIN_TARGET_WIDTH: usize = 8;

#[derive(Debug, Clone)]
struct Slot {
    target: Target,
    state: SlotState,
    history: Vec<Duration>,
    last_latency: Option<Duration>,
}

#[derive(Debug, Clone)]
enum SlotState {
    Pending,
    Attempting { attempt: u32, started: Instant },
    Ready { took: Duration },
    Failed { took: Duration, stages: Vec<Stage> },
}

impl SlotState {
    const fn color(&self) -> Color {
        match self {
            Self::Pending | Self::Attempting { .. } => Color::Waiting,
            Self::Ready { .. } => Color::Ready,
            Self::Failed { .. } => Color::Failed,
        }
    }

    const fn label(&self) -> &'static str {
        match self {
            Self::Pending | Self::Attempting { .. } => "waiting",
            Self::Ready { .. } => "ready",
            Self::Failed { .. } => "failed",
        }
    }

    fn took(&self) -> Option<Duration> {
        match self {
            Self::Pending => None,
            Self::Attempting { started, .. } => Some(started.elapsed()),
            Self::Ready { took } | Self::Failed { took, .. } => Some(*took),
        }
    }

    fn suffix(&self) -> Option<String> {
        match self {
            Self::Attempting { attempt, .. } if *attempt > 1 => Some(format!("attempt {attempt}")),
            _ => None,
        }
    }
}

impl Plain {
    pub(crate) fn new(color: bool, quiet: bool) -> Self {
        Self {
            style: Style::new(color),
            quiet,
            states: Vec::new(),
            started: Instant::now(),
            last_lines: 0,
            live: io::stderr().is_terminal(),
            cursor_hidden: false,
            spinner_frame: 0,
        }
    }

    pub(crate) fn tick(&mut self) {
        if self.quiet || !self.live {
            return;
        }
        let active = self
            .states
            .iter()
            .any(|s| matches!(s.state, SlotState::Pending | SlotState::Attempting { .. }));
        if !active {
            return;
        }
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
        self.redraw();
    }

    #[must_use]
    pub(crate) const fn tick_interval() -> Duration {
        Duration::from_millis(theme::SPINNER_INTERVAL_MS)
    }

    pub(crate) fn banner(&mut self, targets: &[Target], cmd: Option<&[String]>) {
        if self.quiet {
            return;
        }
        self.started = Instant::now();
        self.states = targets
            .iter()
            .cloned()
            .map(|target| Slot {
                target,
                state: SlotState::Pending,
                history: Vec::with_capacity(SPARK_HISTORY),
                last_latency: None,
            })
            .collect();
        let echo = render_command(targets, cmd);
        let _ = writeln!(
            io::stderr(),
            "{} {}",
            self.style.paint(theme::DOLLAR, Color::Subtle),
            self.style.paint(&echo, Color::Text),
        );
        self.redraw();
    }

    pub(crate) fn handle(&mut self, ev: &Event) {
        if self.quiet {
            return;
        }
        #[allow(clippy::wildcard_enum_match_arm)]
        match ev {
            Event::Attempt {
                idx,
                target,
                attempt,
                latency,
                ..
            } => {
                if let Some(slot) = self.states.get_mut(*idx) {
                    slot.target = target.clone();
                    if slot.history.len() == SPARK_HISTORY {
                        slot.history.remove(0);
                    }
                    slot.history.push(*latency);
                    slot.last_latency = Some(*latency);
                    let started = match &slot.state {
                        SlotState::Attempting { started, .. } => *started,
                        _ => Instant::now(),
                    };
                    slot.state = SlotState::Attempting {
                        attempt: *attempt,
                        started,
                    };
                }
                if self.live {
                    self.redraw();
                }
            }
            Event::Finished {
                idx,
                target,
                outcome,
                satisfied,
                ..
            } => {
                if let Some(slot) = self.states.get_mut(*idx) {
                    slot.target = target.clone();
                    slot.state = if *satisfied {
                        SlotState::Ready {
                            took: outcome.total,
                        }
                    } else {
                        SlotState::Failed {
                            took: outcome.total,
                            stages: outcome.stages.clone(),
                        }
                    };
                }
                if self.live {
                    self.redraw();
                } else if let Some(slot) = self.states.get(*idx) {
                    let _ = writeln!(io::stderr(), "{}", self.format_slot(slot));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn summary(&mut self, report: &Report, exec: Option<&[String]>) {
        if self.quiet {
            return;
        }
        self.redraw();

        for r in &report.results {
            if !r.satisfied {
                self.print_diagnostic(r);
            }
        }

        let mut out = io::stderr().lock();
        let arrow = self.style.paint(theme::ARROW, Color::Subtle);
        if report.all_ready() {
            let head = self.style.paint("all targets ready", Color::Ready);
            let body = if let Some(cmd) = exec.filter(|c| !c.is_empty()) {
                let joined = cmd.join(" ");
                format!(
                    " {} exec {}",
                    self.style.paint(theme::SEP, Color::Subtle),
                    self.style.paint(&joined, Color::Text),
                )
            } else {
                String::new()
            };
            let _ = writeln!(out, "{arrow} {head}{body}");
        } else {
            let ready = report.results.iter().filter(|r| r.satisfied).count();
            let total = report.results.len();
            let line = format!("{ready}/{total} ready");
            let head = self.style.paint(&line, Color::Failed);
            let elapsed = fmt_dur(report.elapsed);
            let _ = writeln!(
                out,
                "{arrow} {head} {} {}",
                self.style.paint(theme::SEP, Color::Subtle),
                self.style.paint(&elapsed, Color::Subtle)
            );
        }
    }

    fn redraw(&mut self) {
        if !self.live {
            return;
        }
        let mut out = io::stderr().lock();
        if !self.cursor_hidden {
            let _ = queue!(out, cursor::Hide);
            self.cursor_hidden = true;
        }
        if self.last_lines > 0 {
            let _ = queue!(
                out,
                cursor::MoveUp(self.last_lines),
                terminal::Clear(terminal::ClearType::FromCursorDown),
            );
        }
        let mut lines = 0u16;
        for slot in &self.states {
            let _ = writeln!(out, "{}", self.format_slot(slot));
            lines = lines.saturating_add(1);
        }
        let _ = out.flush();
        self.last_lines = lines;
    }

    fn format_slot(&self, slot: &Slot) -> String {
        let last = if slot.history.len() >= 2 {
            slot.last_latency
        } else {
            None
        };
        let head: &str = match slot.state {
            SlotState::Pending | SlotState::Attempting { .. } => {
                theme::SPINNER[self.spinner_frame % theme::SPINNER.len()]
            }
            SlotState::Ready { .. } => theme::CHECK,
            SlotState::Failed { .. } => theme::CROSS,
        };
        self.format_line(
            head,
            slot.state.color(),
            slot.state.label(),
            &slot.target,
            slot.state.suffix().as_deref(),
            slot.state.took(),
            &slot.history,
            last,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn format_line(
        &self,
        head: &str,
        tone: Color,
        word: &str,
        target: &Target,
        suffix: Option<&str>,
        took: Option<Duration>,
        history: &[Duration],
        last_latency: Option<Duration>,
    ) -> String {
        let cols = terminal_width();
        let took_str = took.map(fmt_dur);
        let last_str = last_latency.map(fmt_dur);
        let spark = sparkline(history, theme::SPARK);
        let sep_w = theme::SEP.chars().count();

        let head_w = head.chars().count();
        let extra_w = |s: &str| 3 + sep_w + s.chars().count();
        let fixed_visible: usize = head_w
            + 1
            + word.chars().count()
            + 1
            + suffix.map_or(0, |s| 1 + s.chars().count())
            + took_str.as_deref().map_or(0, extra_w)
            + if spark.is_empty() { 0 } else { extra_w(&spark) }
            + last_str.as_deref().map_or(0, extra_w);

        let target_str = target.to_string();
        let max_target = cols.saturating_sub(fixed_visible).max(MIN_TARGET_WIDTH);
        let target_trunc = truncate_to(&target_str, max_target, theme::ELLIPSIS);

        let brand_painted = self.style.paint(head, tone);
        let label = self.style.paint(word, tone);
        let target_painted = self.style.paint(&target_trunc, Color::Text);
        let sep = self.style.paint(theme::SEP, Color::Subtle);

        let mut line = format!("{brand_painted} {label} {target_painted}");
        if let Some(s) = suffix {
            line.push(' ');
            line.push_str(&self.style.paint(s, Color::Subtle));
        }
        if let Some(d) = took_str.as_deref() {
            line.push(' ');
            line.push_str(&sep);
            line.push(' ');
            line.push_str(&self.style.paint(d, Color::Subtle));
        }
        if !spark.is_empty() {
            line.push(' ');
            line.push_str(&sep);
            line.push(' ');
            line.push_str(&self.style.paint(&spark, Color::Subtle));
        }
        if let Some(d) = last_str.as_deref() {
            line.push(' ');
            line.push_str(&sep);
            line.push(' ');
            line.push_str(&self.style.paint(d, Color::Subtle));
        }
        line
    }

    fn print_diagnostic(&self, r: &TargetReport) {
        let from_state = self.states.iter().find_map(|s| match &s.state {
            SlotState::Failed { stages, .. } if same_target(&s.target, &r.target) => {
                Some(stages.as_slice())
            }
            _ => None,
        });
        self.print_stages(from_state.unwrap_or(&r.final_outcome.stages));
    }

    fn print_stages(&self, stages: &[Stage]) {
        let mut out = io::stderr().lock();
        let mut hint_to_print: Option<String> = None;
        for (i, stage) in stages.iter().enumerate() {
            let last = i + 1 == stages.len();
            let prefix = self
                .style
                .paint(if last { theme::ELL } else { theme::TEE }, Color::Subtle);
            let kind = self.style.paint(stage.kind.as_str(), Color::Text);
            if let StageResult::Err { message, hint } = &stage.result {
                let _ = writeln!(
                    out,
                    "{prefix} {kind} {} {}",
                    self.style.paint(theme::CROSS, Color::Failed),
                    self.style.paint(message, Color::Text),
                );
                if let Some(h) = hint {
                    hint_to_print = Some(h.to_string());
                }
            } else {
                let took = fmt_dur(stage.took);
                let _ = writeln!(
                    out,
                    "{prefix} {kind} {} {}",
                    self.style.paint(theme::CHECK, Color::Ready),
                    self.style.paint(&took, Color::Subtle),
                );
            }
        }
        if let Some(h) = hint_to_print {
            let _ = writeln!(
                out,
                "{} {}",
                self.style.paint("hint:", Color::Subtle),
                self.style.paint(&h, Color::Text),
            );
        }
    }
}

impl Drop for Plain {
    fn drop(&mut self) {
        if self.cursor_hidden {
            let _ = execute!(io::stderr(), cursor::Show);
        }
    }
}

fn sparkline(values: &[Duration], blocks: &[char]) -> String {
    if values.len() < 2 || blocks.is_empty() {
        return String::new();
    }
    let max = values.iter().max().copied().unwrap_or(Duration::ZERO);
    let min = values.iter().min().copied().unwrap_or(Duration::ZERO);
    let max_us = max.as_micros();
    let min_us = min.as_micros();
    let range = max_us.saturating_sub(min_us);
    let last_idx = blocks.len() - 1;
    values
        .iter()
        .map(|d| {
            if range == 0 {
                blocks[last_idx / 2]
            } else {
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_sign_loss,
                    clippy::cast_possible_truncation
                )]
                let idx = {
                    let span = (d.as_micros().saturating_sub(min_us)) as f64 / range as f64;
                    ((span * last_idx as f64).round() as usize).min(last_idx)
                };
                blocks[idx]
            }
        })
        .collect()
}

fn terminal_width() -> usize {
    terminal::size().map_or(DEFAULT_TERMINAL_WIDTH, |(c, _)| c as usize)
}

fn truncate_to<'a>(s: &'a str, max: usize, ellipsis: &str) -> Cow<'a, str> {
    if s.chars().count() <= max {
        return Cow::Borrowed(s);
    }
    if max <= 1 {
        return Cow::Owned(ellipsis.to_owned());
    }
    let take = max.saturating_sub(1);
    let mut out: String = s.chars().take(take).collect();
    out.push_str(ellipsis);
    Cow::Owned(out)
}

fn render_command(targets: &[Target], exec: Option<&[String]>) -> String {
    let mut parts: Vec<String> = vec!["holdon".into()];
    parts.extend(targets.iter().map(ToString::to_string));
    if let Some(cmd) = exec.filter(|c| !c.is_empty()) {
        parts.push("--".into());
        parts.extend(cmd.iter().cloned());
    }
    parts.join(" ")
}

fn same_target(a: &Target, b: &Target) -> bool {
    a.to_string() == b.to_string()
}
