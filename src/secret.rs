//! Compile-time-free secret resolution for target strings.
//!
//! Recognised placeholders, in order of precedence:
//!
//! | Syntax            | Replacement                                  |
//! |-------------------|----------------------------------------------|
//! | `${{env:NAME}}`   | value of the `NAME` environment variable     |
//! | `${{file:PATH}}`  | contents of `PATH`, trailing newline trimmed |
//!
//! Substitution happens before URL parsing so any character allowed in
//! a target string can come from a secret source. The resolved value is
//! not percent-encoded; callers must apply their own encoding when the
//! result is embedded in URL userinfo and may contain reserved
//! characters.
//!
//! Missing env vars, unreadable files, and unknown source kinds all
//! produce a precise error at startup so a deploy gate fails fast
//! instead of trying to dial a literal placeholder host.
//!
//! Only the two kinds above are recognised. A literal `$ {` that does
//! not match a known source kind is left untouched, so this never
//! breaks targets that happen to contain shell-style braces unrelated
//! to secret resolution.

use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Hard cap on the size of a file referenced by `${file:PATH}`. Anything
/// larger is refused at metadata-check time so a stray placeholder
/// pointing at /var/log can never grow the target string to gigabytes.
const MAX_SECRET_FILE_BYTES: u64 = 64 * 1024;

/// Walk the input once, replacing each `${env:...}` and `${file:...}`
/// placeholder with the resolved value. Returns the original borrow
/// when no placeholder is present so the common case is allocation-free.
pub(crate) fn resolve(input: &str) -> Result<Cow<'_, str>> {
    if !input.contains("${") {
        return Ok(Cow::Borrowed(input));
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    let bytes = input.as_bytes();
    while cursor < bytes.len() {
        if !input[cursor..].starts_with("${") {
            // Append the next chunk up to the next `${` (or to end).
            let end = input[cursor..]
                .find("${")
                .map_or(bytes.len(), |off| cursor + off);
            out.push_str(&input[cursor..end]);
            cursor = end;
            continue;
        }
        // Find the LAST `}` between this `${` and the next `${`, so
        // file paths containing `}` survive intact. Env names cannot
        // contain `}` anyway, so being greedy is safe for both kinds.
        let search_end = input[cursor + 2..]
            .find("${")
            .map_or(input.len(), |off| cursor + 2 + off);
        let Some(rel_end) = input[cursor + 2..search_end].rfind('}') else {
            // No closing brace before the next `${` (or end of input).
            // Could be an unknown-kind placeholder whose argument
            // contains a literal `${`. Per the module contract we leave
            // unknown placeholders alone, so emit `${` verbatim and
            // continue scanning. Only bail for the truly unterminated
            // case where there is no `}` anywhere after the cursor.
            if !input[cursor + 2..].contains('}') {
                bail!("unterminated placeholder in target string at byte offset {cursor}");
            }
            out.push_str("${");
            cursor += 2;
            continue;
        };
        let close = cursor + 2 + rel_end;
        let body = &input[cursor + 2..close];
        let Some((kind, arg)) = body.split_once(':') else {
            // Not a `kind:arg` shape, leave the entire `${...}` alone.
            out.push_str(&input[cursor..=close]);
            cursor = close + 1;
            continue;
        };
        let resolved = match kind {
            "env" => resolve_env(arg)?,
            "file" => resolve_file(arg)?,
            _ => {
                // Unknown kind, leave as-is so unrelated `${something:x}`
                // strings flow through untouched.
                out.push_str(&input[cursor..=close]);
                cursor = close + 1;
                continue;
            }
        };
        out.push_str(&resolved);
        cursor = close + 1;
    }
    Ok(Cow::Owned(out))
}

fn resolve_env(name: &str) -> Result<String> {
    if name.is_empty() {
        bail!("env placeholder requires a variable name");
    }
    std::env::var(name).with_context(|| format!("reading env var `{name}`"))
}

fn resolve_file(path: &str) -> Result<String> {
    if path.is_empty() {
        bail!("file placeholder requires a path");
    }
    let metadata = std::fs::metadata(Path::new(path))
        .with_context(|| format!("reading secret file `{path}`"))?;
    // Refuse character devices, pipes, sockets etc. Their reported
    // length is 0 on Linux/macOS, which would otherwise sail past the
    // size cap below and trap us in an unbounded read.
    if !metadata.is_file() {
        bail!("secret file `{path}` is not a regular file");
    }
    if metadata.len() > MAX_SECRET_FILE_BYTES {
        bail!(
            "secret file `{path}` is {} bytes, max {} (use a separate file or pre-process the secret)",
            metadata.len(),
            MAX_SECRET_FILE_BYTES
        );
    }
    let raw = std::fs::read_to_string(Path::new(path))
        .with_context(|| format!("reading secret file `{path}`"))?;
    // Trim a single trailing newline so a file written by `echo`
    // produces the same value the user expects to interpolate.
    let trimmed = raw.strip_suffix('\n').unwrap_or(&raw);
    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
    Ok(trimmed.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::literal_string_with_formatting_args)]
mod tests {
    use super::*;
    use std::io::Write as _;

    // Env-based tests live in tests/integration_secret_resolver.rs
    // because std::env::set_var is unsafe in edition 2024 and the bin
    // crate forbids unsafe. Integration tests get their own process so
    // PATH/env mutations from the test harness do not race.

    #[test]
    fn passes_through_without_placeholder() {
        let r = resolve("tcp://host:5432").unwrap();
        assert!(matches!(r, Cow::Borrowed(_)));
        assert_eq!(r, "tcp://host:5432");
    }

    #[test]
    fn missing_env_var_is_error() {
        let err = resolve("${env:HOLDON_TEST_DEFINITELY_MISSING}").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("HOLDON_TEST_DEFINITELY_MISSING"), "{msg}");
    }

    #[test]
    fn file_secret_substituted_and_newline_trimmed() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"sekrit\n").unwrap();
        let path = f.path().to_string_lossy().into_owned();
        let input = format!("redis://x:${{file:{path}}}@h:6379");
        let r = resolve(&input).unwrap();
        assert_eq!(r, "redis://x:sekrit@h:6379");
    }

    #[test]
    fn unknown_kind_left_untouched() {
        let r = resolve("scheme://x/${vault:foo}/y").unwrap();
        assert_eq!(r, "scheme://x/${vault:foo}/y");
    }

    #[test]
    fn unknown_kind_with_nested_brace_is_untouched() {
        // Unknown placeholder kinds must pass through even when their
        // argument syntax happens to embed another `${`.
        let r = resolve("scheme://x/${vault:ns/${key}}/y").unwrap();
        assert_eq!(r, "scheme://x/${vault:ns/${key}}/y");
    }

    #[test]
    fn unterminated_brace_is_error() {
        let err = resolve("scheme://x/${env:NAME_NO_CLOSE").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unterminated"), "{msg}");
    }

    #[test]
    fn file_path_with_braces_resolves_last_brace() {
        let mut dir = tempfile::TempDir::new().unwrap();
        let _ = &mut dir;
        let path = dir.path().join("with{braces}");
        std::fs::write(&path, b"ok").unwrap();
        let p = path.to_string_lossy().into_owned();
        let input = format!("redis://x:${{file:{p}}}@h");
        let r = resolve(&input).unwrap();
        assert_eq!(r, "redis://x:ok@h");
    }

    #[test]
    fn oversized_file_rejected() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let size = usize::try_from(MAX_SECRET_FILE_BYTES + 1).unwrap();
        let big = vec![b'a'; size];
        f.write_all(&big).unwrap();
        let p = f.path().to_string_lossy().into_owned();
        let err = resolve(&format!("redis://x:${{file:{p}}}@h")).unwrap_err();
        assert!(format!("{err:#}").contains("max"));
    }

    #[test]
    fn empty_file_path_is_error() {
        let err = resolve("scheme://x/${file:}").unwrap_err();
        assert!(format!("{err:#}").contains("requires a path"));
    }

    #[test]
    fn empty_env_name_is_error() {
        let err = resolve("scheme://x/${env:}").unwrap_err();
        assert!(format!("{err:#}").contains("requires a variable name"));
    }

    #[test]
    fn file_missing_is_error() {
        let r = resolve("redis://x:${file:/no/such/file/anywhere}@h");
        assert!(r.is_err());
    }
}
