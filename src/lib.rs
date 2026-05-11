#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]

//! Public Rust API for the `holdon` wait-for-readiness tool.
//!
//! The CLI binary is the primary surface. This library exposes the same probe
//! engine for programmatic use. Most callers want the [`Runner`] and [`Target`]
//! pair: build a config, parse targets, drive the runner, inspect the
//! resulting [`Report`].

#[doc(hidden)]
pub mod checker;
/// Per-stage diagnostic types produced by every probe.
pub mod diagnostic;
/// Error types returned by parsing and probing.
pub mod error;
/// [`Runner`] orchestration: scheduling, retries, reporting.
pub mod runner;
/// [`Target`] enum and URL parsing.
pub mod target;
/// Utility helpers re-exported for downstream consumers.
pub mod util;

pub use error::{Error, Result};
pub use runner::{Direction, Event, Report, Runner, RunnerConfig, Schedule, TargetReport};
pub use target::{Hostname, LogMatcher, Target};
pub use util::parse_duration;
