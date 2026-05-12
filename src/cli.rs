use std::time::Duration;

use clap::{CommandFactory, Parser, ValueEnum};
#[cfg(feature = "http")]
use holdon::checker::http::{HeaderName, HeaderValue, Method};

use crate::output::Format;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

impl From<CompletionShell> for clap_complete::Shell {
    fn from(s: CompletionShell) -> Self {
        match s {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Zsh => Self::Zsh,
            CompletionShell::Fish => Self::Fish,
            CompletionShell::PowerShell => Self::PowerShell,
            CompletionShell::Elvish => Self::Elvish,
        }
    }
}

pub(crate) fn print_completion(shell: CompletionShell) {
    let mut cmd = Args::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(
        clap_complete::Shell::from(shell),
        &mut cmd,
        name,
        &mut std::io::stdout(),
    );
}

pub(crate) fn print_manpage() -> std::io::Result<()> {
    let cmd = Args::command();
    let man = clap_mangen::Man::new(cmd);
    man.render(&mut std::io::stdout())
}

#[cfg(feature = "http")]
#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq, Eq)]
pub(crate) enum HttpMethod {
    #[default]
    Get,
    Head,
    Post,
    Put,
    Delete,
    Options,
}

#[cfg(feature = "http")]
impl From<HttpMethod> for Method {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::Get => Self::GET,
            HttpMethod::Head => Self::HEAD,
            HttpMethod::Post => Self::POST,
            HttpMethod::Put => Self::PUT,
            HttpMethod::Delete => Self::DELETE,
            HttpMethod::Options => Self::OPTIONS,
        }
    }
}

#[cfg(feature = "http")]
const SENSITIVE_HEADER_NAMES: &[&str] = &[
    "authorization",
    "cookie",
    "proxy-authorization",
    "x-api-key",
    "x-auth-token",
    "x-amz-security-token",
];

#[cfg(feature = "http")]
#[derive(Clone)]
pub(crate) struct HeaderPair {
    pub(crate) name: HeaderName,
    pub(crate) value: HeaderValue,
}

#[cfg(feature = "http")]
impl std::fmt::Debug for HeaderPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.name.as_str();
        let redacted = SENSITIVE_HEADER_NAMES
            .iter()
            .any(|s| s.eq_ignore_ascii_case(n));
        if redacted {
            write!(f, "{n}: ***")
        } else {
            write!(f, "{n}: {:?}", self.value)
        }
    }
}

#[cfg(feature = "http")]
fn parse_header_pair(input: &str) -> Result<HeaderPair, String> {
    let (name, value) = holdon::checker::http::parse_header(input)?;
    Ok(HeaderPair { name, value })
}

#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    #[default]
    Plain,
    Quiet,
    #[cfg(feature = "json-output")]
    Json,
}

impl From<OutputFormat> for Format {
    fn from(o: OutputFormat) -> Self {
        match o {
            OutputFormat::Plain => Self::Plain,
            OutputFormat::Quiet => Self::Quiet,
            #[cfg(feature = "json-output")]
            OutputFormat::Json => Self::Json,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "holdon",
    version,
    about = "Wait for anything. Know why if it doesn't.",
    long_about = None,
    after_help = "Examples:\n  holdon :5432\n  holdon :5432 :6379 -- npm run migrate\n  holdon https://api.local/health --timeout 60s",
    arg_required_else_help = true,
)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Args {
    #[arg(value_name = "TARGET")]
    pub(crate) targets: Vec<String>,

    #[arg(
        long,
        value_name = "PATH",
        env = "HOLDON_CONFIG",
        help = "Load defaults and additional targets from a TOML file"
    )]
    pub(crate) config: Option<std::path::PathBuf>,

    #[arg(
        long = "generate-completion",
        value_name = "SHELL",
        value_enum,
        hide = true,
        help = "Print a shell completion script to stdout and exit"
    )]
    pub(crate) generate_completion: Option<CompletionShell>,

    #[arg(
        long = "generate-manpage",
        hide = true,
        help = "Print the man page to stdout and exit"
    )]
    pub(crate) generate_manpage: bool,

    #[arg(long, short = 't', env = "HOLDON_TIMEOUT", value_parser = holdon::parse_duration, default_value = "30s")]
    pub(crate) timeout: Duration,

    #[arg(long, env = "HOLDON_INTERVAL", value_parser = holdon::parse_duration, default_value = "100ms")]
    pub(crate) interval: Duration,

    #[arg(long, env = "HOLDON_MAX_INTERVAL", value_parser = holdon::parse_duration, default_value = "2s")]
    pub(crate) max_interval: Duration,

    #[arg(long, env = "HOLDON_INITIAL_DELAY", value_parser = holdon::parse_duration, default_value = "0s")]
    pub(crate) initial_delay: Duration,

    #[arg(long, env = "HOLDON_ATTEMPT_TIMEOUT", value_parser = holdon::parse_duration, default_value = "5s")]
    pub(crate) attempt_timeout: Duration,

    #[arg(long, short = 'q', env = "HOLDON_QUIET")]
    pub(crate) quiet: bool,

    #[arg(long, value_enum, env = "HOLDON_OUTPUT", default_value_t = OutputFormat::Plain)]
    pub(crate) output: OutputFormat,

    #[arg(long, env = "HOLDON_REVERSE")]
    pub(crate) reverse: bool,

    #[arg(long, env = "HOLDON_ONCE")]
    pub(crate) once: bool,

    #[arg(long, short = 's', env = "HOLDON_STRICT")]
    pub(crate) strict: bool,

    #[arg(long, env = "HOLDON_SEQUENTIAL")]
    pub(crate) sequential: bool,

    #[arg(long, env = "HOLDON_SUCCESS_THRESHOLD",
          default_value_t = holdon::RunnerConfig::DEFAULT_SUCCESS_THRESHOLD)]
    pub(crate) success_threshold: u32,

    #[arg(long, env = "HOLDON_AT_LEAST")]
    pub(crate) at_least: Option<usize>,

    #[arg(long, env = "HOLDON_EXPECT_STATUS", value_parser = parse_status_range)]
    pub(crate) expect_status: Option<(u16, u16)>,

    #[arg(long, env = "HOLDON_NO_JITTER", action = clap::ArgAction::SetTrue)]
    pub(crate) no_jitter: bool,

    #[arg(long, env = "HOLDON_TIMEOUT_EXIT_CODE", default_value_t = crate::DEFAULT_TIMEOUT_EXIT_CODE)]
    pub(crate) timeout_exit_code: u8,

    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub(crate) no_color: bool,

    #[arg(long, short = 'v', action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    #[cfg(feature = "http")]
    #[arg(long = "header", short = 'H', value_parser = parse_header_pair,
          help = "Extra HTTP request header `Name: Value` (repeatable)")]
    pub(crate) headers: Vec<HeaderPair>,

    #[cfg(feature = "http")]
    #[arg(long, value_enum, default_value_t = HttpMethod::Get,
          help = "HTTP method for http(s):// probes")]
    pub(crate) method: HttpMethod,

    #[cfg(feature = "http")]
    #[arg(
        long,
        value_name = "BODY",
        help = "Request body for http(s):// probes (sent as application/octet-stream unless overridden by -H)"
    )]
    pub(crate) data: Option<String>,

    #[cfg(feature = "http")]
    #[arg(long, help = "Skip TLS certificate verification (dev only)")]
    pub(crate) insecure: bool,

    #[cfg(feature = "http")]
    #[arg(long, help = "Substring that must appear in the HTTP response body")]
    pub(crate) expect_body: Option<String>,

    #[cfg(feature = "http")]
    #[arg(
        long,
        value_name = "PATTERN",
        value_parser = parse_body_regex,
        help = "Regex the HTTP response body must match"
    )]
    pub(crate) expect_body_regex: Option<regex_lite::Regex>,

    #[cfg(feature = "http")]
    #[arg(
        long,
        value_name = "POINTER=VALUE",
        value_parser = parse_json_match,
        help = "Require JSON pointer POINTER (RFC 6901) to equal VALUE (e.g. /status=UP)"
    )]
    pub(crate) expect_json: Option<(String, String)>,

    #[cfg(feature = "http")]
    #[arg(
        long,
        help = "Do not follow HTTP redirects; report the first response status"
    )]
    pub(crate) no_follow_redirects: bool,

    #[cfg(feature = "http")]
    #[arg(
        long,
        value_name = "PATH",
        help = "Append PEM CA certificate(s) from PATH to the bundled webpki roots"
    )]
    pub(crate) ca_cert: Option<std::path::PathBuf>,

    #[cfg(feature = "http")]
    #[arg(long, value_enum, default_value_t = TlsMinArg::V12,
          help = "Minimum TLS version for HTTPS probes")]
    pub(crate) tls_min: TlsMinArg,

    #[arg(last = true)]
    pub(crate) exec: Vec<String>,
}

#[cfg(feature = "http")]
#[derive(Debug, Clone, Copy, ValueEnum, Default, PartialEq, Eq)]
pub(crate) enum TlsMinArg {
    #[default]
    #[value(name = "1.2")]
    V12,
    #[value(name = "1.3")]
    V13,
}

#[cfg(feature = "http")]
impl From<TlsMinArg> for holdon::checker::http::TlsMin {
    fn from(v: TlsMinArg) -> Self {
        match v {
            TlsMinArg::V12 => Self::V12,
            TlsMinArg::V13 => Self::V13,
        }
    }
}

#[cfg(feature = "http")]
fn parse_body_regex(input: &str) -> Result<regex_lite::Regex, String> {
    regex_lite::Regex::new(input).map_err(|e| format!("invalid regex: {e}"))
}

#[cfg(feature = "http")]
fn parse_json_match(input: &str) -> Result<(String, String), String> {
    let (pointer, value) = input
        .split_once('=')
        .ok_or_else(|| "expected `POINTER=VALUE`".to_owned())?;
    if pointer.is_empty() {
        return Err("json pointer cannot be empty".into());
    }
    if !pointer.starts_with('/') {
        return Err("json pointer must start with `/` (RFC 6901)".into());
    }
    Ok((pointer.to_owned(), value.to_owned()))
}

fn parse_status_range(s: &str) -> Result<(u16, u16), String> {
    if let Some((lo, hi)) = s.split_once('-') {
        let lo: u16 = lo.parse().map_err(|e| format!("bad lo: {e}"))?;
        let hi: u16 = hi.parse().map_err(|e| format!("bad hi: {e}"))?;
        if hi < lo {
            return Err(format!("hi {hi} < lo {lo}"));
        }
        Ok((lo, hi))
    } else {
        let n: u16 = s.parse().map_err(|e| format!("bad status: {e}"))?;
        Ok((n, n))
    }
}
