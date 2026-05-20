#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(missing_debug_implementations, unused_must_use, rust_2018_idioms)]

#[doc(hidden)]
pub mod checker;
pub mod diagnostic;
pub mod error;
pub mod runner;
pub mod target;
pub mod util;

pub use error::{Error, Result};
pub use runner::{Direction, Event, Report, Runner, RunnerConfig, Schedule, TargetReport};
pub use target::{Hostname, LogMatcher, ProcessSelector, RedisKeyExpect, Target};
pub use util::parse_duration;
