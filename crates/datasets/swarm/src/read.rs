//! Reading `redacted.jsonl[.gz]`.

use std::fs;
use std::io::Read;
use std::path::Path;

use a2a_bench_corpus::export::FilesRead;
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::PAYLOADS_FILE;
use crate::error::{SwarmError, category};

/// One row of the redacted export. Unknown fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Row {
    pub id: String,
    /// `payload`, `recovered_text` or `response`.
    pub kind: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub text: String,
}

/// The rows of `redacted.jsonl.gz` (preferred) or `redacted.jsonl` under
/// `root`, in file order, recording the file read in `files`.
pub fn rows(root: &Path, files: &mut FilesRead) -> Result<Vec<Row>, SwarmError> {
    let gz_name = format!("{PAYLOADS_FILE}.gz");
    let gz = root.join(&gz_name);
    let plain = root.join(PAYLOADS_FILE);
    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| SwarmError::Io { path, source }
    };
    let bytes = if gz.exists() {
        let raw = fs::read(&gz).map_err(io(&gz))?;
        let mut out = Vec::with_capacity(raw.len().saturating_mul(8));
        GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .map_err(io(&gz))?;
        files.record(gz_name);
        out
    } else if plain.exists() {
        let out = fs::read(&plain).map_err(io(&plain))?;
        files.record(PAYLOADS_FILE);
        out
    } else {
        return Err(SwarmError::Missing {
            root: root.display().to_string(),
            file: PAYLOADS_FILE.to_owned(),
        });
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for (at, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row = serde_json::from_str(line).map_err(|error| SwarmError::Json {
            path: PAYLOADS_FILE.to_owned(),
            line: at + 1,
            column: error.column(),
            category: category(&error),
        })?;
        out.push(row);
    }
    Ok(out)
}
