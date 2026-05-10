use std::io;

use thiserror::Error;

/// Result alias defaulting to [`enum@Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by the public API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Target string was recognizable in shape but malformed.
    #[error("invalid target `{input}`: {reason}")]
    Parse {
        /// Original input string.
        input: String,
        /// Reason the input was rejected.
        reason: String,
    },

    /// One or more targets failed to satisfy their readiness condition.
    #[error("{failed} of {total} targets failed to become ready")]
    NotReady {
        /// Number of targets that failed.
        failed: usize,
        /// Total number of targets in the report.
        total: usize,
    },

    /// Target shorthand carried no port and a port was required.
    #[error("missing port in `{0}`")]
    MissingPort(String),

    /// URL parsed cleanly but its scheme is not supported by this build.
    #[error("unsupported scheme `{0}`")]
    UnsupportedScheme(String),

    /// Underlying I/O failure (DNS, socket, filesystem).
    #[error("io: {0}")]
    Io(#[from] io::Error),

    /// Malformed URL rejected by the [`url`] crate.
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    /// Wraps a [`reqwest`] failure. Present only with the `http` feature.
    #[cfg(feature = "http")]
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
}
