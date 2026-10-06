//! The detector's input view of an export: `manifest.json` without the
//! labels digest, `messages.jsonl` and `exchanges.jsonl`. Never
//! `labels.jsonl`.

use std::path::Path;

use a2a_bench_format::manifest::Manifest;

use super::write::{EXCHANGES_FILE, MANIFEST_FILE, MESSAGES_FILE, read_manifest, write_manifest};
use super::{ExportError, empty_dir};

/// Hard-links `from` to `to`, or copies it when a link cannot be made
/// (another filesystem, no link support).
fn link_or_copy(from: &Path, to: &Path) -> Result<(), ExportError> {
    match std::fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::debug!(from = %from.display(), to = %to.display(), error = %error, "hard link failed; copying");
            std::fs::copy(from, to)
                .map(|_| ())
                .map_err(|source| ExportError::Io {
                    path: to.to_path_buf(),
                    source,
                })
        }
    }
}

/// Builds the input view of the export in `export_dir` in `dest`, which
/// must be empty or new: the manifest's [`Manifest::input_view`], and the
/// messages and exchanges files hard-linked (or copied). Returns the view's
/// manifest.
pub fn input_view(export_dir: &Path, dest: &Path) -> Result<Manifest, ExportError> {
    if !export_dir.join(MANIFEST_FILE).is_file() {
        return Err(ExportError::NotAnExport(export_dir.to_path_buf()));
    }
    let view = read_manifest(export_dir)?.input_view();
    empty_dir(dest)?;
    for name in [MESSAGES_FILE, EXCHANGES_FILE] {
        link_or_copy(&export_dir.join(name), &dest.join(name))?;
    }
    write_manifest(dest, &view)?;
    tracing::info!(export = %export_dir.display(), view = %dest.display(), "input view built");
    Ok(view)
}
