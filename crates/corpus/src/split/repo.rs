//! Whether a directory lies inside a git repository.

use std::path::{Path, PathBuf};

use super::SplitError;

/// The working tree that holds `path` (a directory with a `.git` entry,
/// directory or file, at or above it), if any. `path` need not exist yet:
/// its nearest existing ancestor is resolved, symlinks included.
pub fn enclosing_repository(path: &Path) -> Result<Option<PathBuf>, SplitError> {
    let locate = |source| SplitError::Locate {
        path: path.to_path_buf(),
        source,
    };
    let absolute = std::path::absolute(path).map_err(locate)?;
    let existing = absolute
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .unwrap_or(&absolute);
    let resolved = existing.canonicalize().map_err(locate)?;
    for directory in resolved.ancestors().chain(absolute.ancestors()) {
        if directory.join(".git").exists() {
            return Ok(Some(directory.to_path_buf()));
        }
    }
    Ok(None)
}
