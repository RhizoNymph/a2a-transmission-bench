//! The data files a selection picks, and their row groups interleaved.

use std::path::Path;

use a2a_bench_corpus::helpers::parquet_rows::row_groups;

use crate::error::LmcacheError;

/// The data files `limit` and `include` pick, relative to `root`, in name
/// order.
pub fn discover(
    root: &Path,
    limit: Option<usize>,
    include: &[String],
) -> Result<Vec<String>, LmcacheError> {
    let data = root.join("data");
    if !data.is_dir() {
        return Err(LmcacheError::NoData {
            root: root.display().to_string(),
        });
    }
    let io = |source| LmcacheError::Io {
        path: data.display().to_string(),
        source,
    };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&data).map_err(io)? {
        let name = entry
            .map_err(io)?
            .file_name()
            .to_string_lossy()
            .into_owned();
        let relative = format!("data/{name}");
        if name.ends_with(".parquet")
            && (include.is_empty()
                || include
                    .iter()
                    .any(|needle| relative.contains(needle.as_str())))
        {
            files.push(relative);
        }
    }
    files.sort();
    if let Some(limit) = limit {
        files.truncate(limit);
    }
    Ok(files)
}

/// One row group of one file: where reading can start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub file: String,
    pub group: usize,
}

/// The row groups of the selected files, interleaved: every file's first
/// group, then every file's second, and so on.
pub fn segments(root: &Path, files: &[String]) -> Result<Vec<Segment>, LmcacheError> {
    let mut per_file = Vec::with_capacity(files.len());
    for file in files {
        per_file.push(row_groups(&root.join(file))?.len());
    }
    let most = per_file.iter().copied().max().unwrap_or(0);
    let mut out = Vec::new();
    for group in 0..most {
        for (file, groups) in files.iter().zip(&per_file) {
            if group < *groups {
                out.push(Segment {
                    file: file.clone(),
                    group,
                });
            }
        }
    }
    Ok(out)
}
