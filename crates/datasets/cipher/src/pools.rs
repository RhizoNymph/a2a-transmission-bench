//! Payload pools: text files with one payload per line
//! (`steganographic-evals/datasets/message_data/*.txt`).

use std::fs;
use std::path::Path;

use crate::error::CipherError;

/// The pools read when `include` names none.
pub const DEFAULT_POOLS: &[&str] = &[
    "random_strs",
    "random_strs_long",
    "sentences_clean",
    "short_phrases",
];

/// One pool: its name (the file stem) and its non-empty lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    pub name: String,
    pub payloads: Vec<String>,
}

impl Pool {
    pub fn parse(name: &str, text: &str) -> Self {
        Self {
            name: name.to_owned(),
            payloads: text
                .lines()
                .map(|line| line.trim_end_matches('\r'))
                .filter(|line| !line.trim().is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    /// The file a pool is read from, relative to the root.
    pub fn file(name: &str) -> String {
        format!("{name}.txt")
    }
}

/// The pools under `root`: those whose stem `include` names exactly or as a
/// substring (the defaults when it names none), in name order, at most
/// `limit` of them, skipping empty ones. Returns the files read too.
pub fn load(
    root: &Path,
    limit: Option<usize>,
    include: &[String],
) -> Result<(Vec<Pool>, Vec<String>), CipherError> {
    let io = |source| CipherError::Io {
        path: root.display().to_string(),
        source,
    };
    let mut stems = Vec::new();
    for entry in fs::read_dir(root).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.extension().is_none_or(|ext| ext != "txt") {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let wanted = if include.is_empty() {
            DEFAULT_POOLS.contains(&stem.as_str())
        } else {
            include.iter().any(|needle| stem.contains(needle.as_str()))
        };
        if wanted {
            stems.push(stem);
        }
    }
    stems.sort();
    if let Some(limit) = limit {
        stems.truncate(limit);
    }
    let mut pools = Vec::with_capacity(stems.len());
    let mut files = Vec::with_capacity(stems.len());
    for stem in stems {
        let file = Pool::file(&stem);
        let path = root.join(&file);
        let text = fs::read_to_string(&path).map_err(|source| CipherError::Io {
            path: path.display().to_string(),
            source,
        })?;
        files.push(file);
        let pool = Pool::parse(&stem, &text);
        if !pool.payloads.is_empty() {
            pools.push(pool);
        }
    }
    if pools.is_empty() {
        return Err(CipherError::NoPools {
            root: root.display().to_string(),
        });
    }
    Ok((pools, files))
}
