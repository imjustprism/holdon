#[cfg(feature = "json-output")]
mod json;
mod plain;
mod style;
mod theme;

use std::time::Duration;

use holdon::Target;
use holdon::runner::{Event, Report};

pub(crate) use plain::Plain;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Format {
    #[default]
    Plain,
    Quiet,
    #[cfg(feature = "json-output")]
    Json,
}

#[derive(Debug)]
pub(crate) enum Printer {
    Plain(Plain),
    Quiet,
    #[cfg(feature = "json-output")]
    Json(json::Json),
}

impl Printer {
    pub(crate) fn new(format: Format, color: bool) -> Self {
        match format {
            Format::Plain => Self::Plain(Plain::new(color)),
            Format::Quiet => Self::Quiet,
            #[cfg(feature = "json-output")]
            Format::Json => Self::Json(json::Json::new()),
        }
    }

    pub(crate) fn banner(&mut self, targets: &[Target], exec: Option<&[String]>) {
        match self {
            Self::Plain(p) => p.banner(targets, exec),
            Self::Quiet => {}
            #[cfg(feature = "json-output")]
            Self::Json(j) => j.banner(targets),
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub(crate) fn handle(&mut self, ev: &Event) {
        match self {
            Self::Plain(p) => p.handle(ev),
            Self::Quiet => {
                let _ = ev;
            }
            #[cfg(feature = "json-output")]
            Self::Json(j) => j.event(ev),
        }
    }

    pub(crate) fn tick(&mut self) {
        if let Self::Plain(p) = self {
            p.tick();
        }
    }

    pub(crate) const fn tick_interval(&self) -> Duration {
        match self {
            Self::Plain(_) => Plain::tick_interval(),
            Self::Quiet => Duration::from_secs(3600),
            #[cfg(feature = "json-output")]
            Self::Json(_) => Duration::from_secs(3600),
        }
    }

    pub(crate) fn summary(&mut self, report: &Report, exec: Option<&[String]>) {
        match self {
            Self::Plain(p) => p.summary(report, exec),
            Self::Quiet => {}
            #[cfg(feature = "json-output")]
            Self::Json(j) => j.summary(report),
        }
    }
}
