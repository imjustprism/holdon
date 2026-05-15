use std::io;

use thiserror::Error;

/// Shorthand for a `Result` whose error defaults to the crate's [`enum@Error`].
///
/// Library users that already track their own error type can pin the second
/// parameter explicitly. The default reduces boilerplate inside the crate
/// where every fallible helper returns the same [`enum@Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by the public API.
///
/// Every variant carries enough context to render a useful one-line failure
/// without consulting the source. The `Parse` variant preserves the original
/// input so error messages can quote the offending CLI argument. Network and
/// HTTP transport errors round-trip through their upstream `From` impls so
/// downstream code can downcast when it needs the original.
///
/// The enum is `#[non_exhaustive]`. Future variants land at the end of the
/// list to keep the existing `From` impls and pattern matches working.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid target `{input}`: {reason}")]
    Parse { input: String, reason: String },

    #[error("{failed} of {total} targets failed to become ready")]
    NotReady { failed: usize, total: usize },

    #[error("missing port in `{0}`")]
    MissingPort(String),

    #[error("unsupported scheme `{0}`")]
    UnsupportedScheme(String),

    #[error("io: {0}")]
    Io(#[from] io::Error),

    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    #[cfg(feature = "http")]
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
}
