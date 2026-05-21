use std::io;

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

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
