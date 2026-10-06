//! Reading the export's JSONL members, gzipped or plain.

use std::fs;
use std::io::Read;
use std::path::Path;

use a2a_bench_corpus::export::FilesRead;
use flate2::read::GzDecoder;
use serde::de::DeserializeOwned;

use crate::error::WikiError;

/// The bytes of `name.gz` (preferred) or `name` under `root`, decompressed,
/// recording the file read in `files`.
pub fn member(root: &Path, name: &str, files: &mut FilesRead) -> Result<Vec<u8>, WikiError> {
    let gz_name = format!("{name}.gz");
    let gz = root.join(&gz_name);
    let plain = root.join(name);
    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| WikiError::Io { path, source }
    };
    if gz.exists() {
        let raw = fs::read(&gz).map_err(io(&gz))?;
        let mut out = Vec::with_capacity(raw.len().saturating_mul(8));
        GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .map_err(io(&gz))?;
        files.record(gz_name);
        Ok(out)
    } else if plain.exists() {
        let out = fs::read(&plain).map_err(io(&plain))?;
        files.record(name);
        Ok(out)
    } else {
        Err(WikiError::Missing {
            root: root.display().to_string(),
            file: name.to_owned(),
        })
    }
}

/// One record per non-blank line of `bytes` (invalid UTF-8 replaced, as
/// crosstalk-eval read it); `file` names the member in errors.
pub fn lines<T: DeserializeOwned>(bytes: &[u8], file: &str) -> Result<Vec<T>, WikiError> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for (at, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(line).map_err(|source| WikiError::Json {
            path: file.to_owned(),
            line: at + 1,
            source,
        })?;
        out.push(value);
    }
    Ok(out)
}
