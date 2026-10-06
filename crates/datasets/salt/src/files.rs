//! Finding trace files: `traces/<experiment>/<condition>/repNNN.json[.gz]`
//! under the dataset root, in a stratified order.
//!
//! Files are listed per condition (sorted), then interleaved round-robin
//! across conditions: every condition's first repetition, then every
//! second one, and so on. A `--limit N` therefore samples as many
//! conditions (and experiments) as it can: `--limit 53` is one file per
//! condition.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Options, SaltError};

fn sorted_dirs(path: &Path) -> Result<Vec<PathBuf>, SaltError> {
    let mut dirs: Vec<PathBuf> = read_dir(path)?
        .into_iter()
        .filter(|entry| entry.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn read_dir(path: &Path) -> Result<Vec<PathBuf>, SaltError> {
    let io = |source| SaltError::Io {
        path: path.display().to_string(),
        source,
    };
    fs::read_dir(path)
        .map_err(io)?
        .map(|entry| entry.map(|entry| entry.path()).map_err(io))
        .collect()
}

fn is_trace(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".json.gz") || name.ends_with(".json"))
}

/// Trace files relative to `root`, stratified, filtered and limited.
pub fn discover(root: &Path, options: &Options) -> Result<Vec<PathBuf>, SaltError> {
    let traces = root.join("traces");
    if !traces.is_dir() {
        return Err(SaltError::NoTraces {
            root: root.display().to_string(),
        });
    }
    let mut per_condition: Vec<Vec<PathBuf>> = Vec::new();
    for experiment in sorted_dirs(&traces)? {
        for condition in sorted_dirs(&experiment)? {
            let mut files: Vec<PathBuf> = read_dir(&condition)?
                .into_iter()
                .filter(|path| path.is_file() && is_trace(path))
                .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
                .filter(|relative| {
                    options.include.is_empty()
                        || options
                            .include
                            .iter()
                            .any(|needle| relative.to_string_lossy().contains(needle.as_str()))
                })
                .collect();
            files.sort();
            if !files.is_empty() {
                per_condition.push(files);
            }
        }
    }
    let longest = per_condition.iter().map(Vec::len).max().unwrap_or(0);
    let mut ordered = Vec::new();
    for round in 0..longest {
        for files in &per_condition {
            if let Some(file) = files.get(round) {
                ordered.push(file.clone());
            }
        }
    }
    if let Some(limit) = options.limit {
        ordered.truncate(limit);
    }
    Ok(ordered)
}

/// A relative path as the source reference writes it: `/`-separated.
pub fn source_file(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

/// The world key of a trace file: its relative path without `traces/` and
/// the extension.
pub fn world_name(relative: &Path) -> String {
    let text = source_file(relative);
    let text = text.strip_prefix("traces/").unwrap_or(&text);
    text.trim_end_matches(".gz")
        .trim_end_matches(".json")
        .to_owned()
}
