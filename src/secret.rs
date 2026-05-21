use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context, Result, bail};

const MAX_SECRET_FILE_BYTES: u64 = 64 * 1024;

pub(crate) fn resolve(input: &str) -> Result<Cow<'_, str>> {
    if !input.contains("${") {
        return Ok(Cow::Borrowed(input));
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    let bytes = input.as_bytes();
    while cursor < bytes.len() {
        if !input[cursor..].starts_with("${") {
            let end = input[cursor..]
                .find("${")
                .map_or(bytes.len(), |off| cursor + off);
            out.push_str(&input[cursor..end]);
            cursor = end;
            continue;
        }
        let Some(rel_end) = input[cursor + 2..].find('}') else {
            bail!("unterminated placeholder in target string at byte offset {cursor}");
        };
        let close = cursor + 2 + rel_end;
        let body = &input[cursor + 2..close];
        let Some((kind, arg)) = body.split_once(':') else {
            out.push_str(&input[cursor..=close]);
            cursor = close + 1;
            continue;
        };
        let resolved = match kind {
            "env" => resolve_env(arg)?,
            "file" => resolve_file(arg)?,
            _ => {
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
    use std::io::Read as _;
    if path.is_empty() {
        bail!("file placeholder requires a path");
    }
    let probe = std::fs::symlink_metadata(Path::new(path))
        .with_context(|| format!("reading secret file `{path}`"))?;
    if !probe.is_file() {
        bail!("secret file `{path}` is not a regular file");
    }
    let mut file = std::fs::File::open(Path::new(path))
        .with_context(|| format!("reading secret file `{path}`"))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("reading secret file `{path}`"))?;
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
    let mut raw = String::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.read_to_string(&mut raw)
        .with_context(|| format!("reading secret file `{path}`"))?;
    let trimmed = raw.strip_suffix('\n').unwrap_or(&raw);
    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
    Ok(trimmed.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::literal_string_with_formatting_args)]
mod tests {
    use super::*;
    use std::io::Write as _;

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
    fn known_kind_with_adjacent_trailing_brace_uses_first_close() {
        let err = resolve("http://h/{${env:HOLDON_TEST_BOGUS_ID}}/p").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("HOLDON_TEST_BOGUS_ID"), "{msg}");
        assert!(!msg.contains("BOGUS_ID}"), "{msg}");
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
