//! The export writer: a [`TraceSource`] to an export directory
//! (`manifest.json`, `messages.jsonl`, `exchanges.jsonl`, `labels.jsonl`).
//!
//! Worlds stream through one at a time: each kept world is written to the
//! three JSONL files in lockstep, so every file holds the worlds in the
//! same order and memory is bounded by the largest world. A world the
//! source fails to produce is logged, recorded in [`Exported::failures`]
//! and skipped; the export goes on. The manifest is written last, with the
//! trailers' digests and each world's exchange count.
//!
//! Exports are deterministic: the same source, info and selection give
//! byte-identical files.

mod digest;
mod input_view;
mod write;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::jsonl::WriteError;
use a2a_bench_format::manifest::{Converter, FileDigests, Manifest, Setting, Source, WorldEntry};
use a2a_bench_format::version::FORMAT;

pub use digest::{DigestError, FilesRead, SOURCE_DIGEST_CONTEXT, source_digest};
pub use input_view::input_view;
pub use write::{
    EXCHANGES_FILE, LABELS_FILE, MANIFEST_FILE, MESSAGES_FILE, read_manifest, write_manifest,
};

use crate::source::TraceSource;
use crate::split::{RELEASE_KEY, SPLIT_DIGEST_KEY, SPLIT_LIST_KEY, Selection, SplitError};

/// Everything the manifest records beside the worlds and digests: what
/// fixes the export's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestInfo {
    pub dataset: DatasetId,
    /// The `n` of `<dataset>@<n>`.
    pub dataset_version: u32,
    pub source: Source,
    pub converter: Converter,
    /// The converter's selection settings (a limit, a stratification). The
    /// split adds its own keys (`split_list`, `split_digest`, `release`).
    pub selection: BTreeMap<String, Setting>,
    /// The pace settings (`Pace::settings`), empty for a dataset that
    /// records its times.
    pub pace: BTreeMap<String, Setting>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Split(#[from] SplitError),
    #[error("the source is dataset {source_dataset}, the manifest info says {info}")]
    DatasetMismatch {
        source_dataset: DatasetId,
        info: DatasetId,
    },
    #[error("world {world} is of dataset {found}, the export is {expected}")]
    WorldDataset {
        world: WorldKey,
        found: DatasetId,
        expected: DatasetId,
    },
    #[error("selection key {0:?} is the split's")]
    ReservedSelectionKey(String),
    #[error("{0} is not empty; an export needs an empty or new directory")]
    NotEmpty(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("writing {file}: {source}")]
    Write {
        file: &'static str,
        source: WriteError,
    },
    #[error("the manifest: {0}")]
    Manifest(serde_json::Error),
    #[error("{0} holds no export manifest")]
    NotAnExport(PathBuf),
}

/// A world the source could not produce.
#[derive(Debug)]
pub struct WorldFailure<E> {
    /// Its position in the source's stream, from 0.
    pub position: usize,
    pub error: E,
}

/// What an export wrote, and what it could not.
#[derive(Debug)]
pub struct Exported<E> {
    pub manifest: Manifest,
    pub failures: Vec<WorldFailure<E>>,
    /// Worlds the source produced that the selection left out.
    pub left_out: usize,
    /// Dev-listed worlds the source never produced (none for a holdout).
    pub missing: Vec<WorldKey>,
}

/// Creates `dir` if needed and checks it is empty.
pub(crate) fn empty_dir(dir: &Path) -> Result<(), ExportError> {
    let io = |source| ExportError::Io {
        path: dir.to_path_buf(),
        source,
    };
    std::fs::create_dir_all(dir).map_err(io)?;
    if std::fs::read_dir(dir).map_err(io)?.next().is_some() {
        return Err(ExportError::NotEmpty(dir.to_path_buf()));
    }
    Ok(())
}

/// Exports the worlds of `source` that `selection` keeps into `out_dir`,
/// which must be empty or new. See the module docs.
///
/// Refused before anything is written: a source of another dataset than
/// `info`'s, a converter selection using the split's keys, a split list cut
/// from another source revision, and a holdout written inside a git
/// repository.
pub fn export<S: TraceSource>(
    source: &mut S,
    out_dir: &Path,
    info: ManifestInfo,
    selection: &Selection,
) -> Result<Exported<S::Error>, ExportError> {
    if source.dataset() != &info.dataset {
        return Err(ExportError::DatasetMismatch {
            source_dataset: source.dataset().clone(),
            info: info.dataset,
        });
    }
    for key in [SPLIT_LIST_KEY, SPLIT_DIGEST_KEY, RELEASE_KEY] {
        if info.selection.contains_key(key) {
            return Err(ExportError::ReservedSelectionKey(key.to_owned()));
        }
    }
    selection.check(&info.source.revision, out_dir)?;
    empty_dir(out_dir)?;

    let filter = selection.filter();
    source.select(&filter);
    let mut writers = write::Writers::create(out_dir, &info.dataset)?;
    let mut worlds = Vec::new();
    let mut failures = Vec::new();
    let mut left_out = 0;
    for (position, world) in source.worlds().enumerate() {
        let world = match world {
            Ok(world) => world,
            Err(error) => {
                tracing::warn!(dataset = %info.dataset, position, error = %error, "world failed to convert; skipped");
                failures.push(WorldFailure { position, error });
                continue;
            }
        };
        if world.dataset() != &info.dataset {
            return Err(ExportError::WorldDataset {
                world: world.key().clone(),
                found: world.dataset().clone(),
                expected: info.dataset,
            });
        }
        if !filter.keeps(world.key()) {
            tracing::debug!(dataset = %info.dataset, world = %world.key(), "world not in the selected split");
            left_out += 1;
            continue;
        }
        writers.world(&world)?;
        tracing::debug!(
            dataset = %info.dataset,
            world = %world.key(),
            exchanges = world.exchanges().len(),
            labels = world.labels().len(),
            "world exported"
        );
        worlds.push(WorldEntry {
            key: world.key().clone(),
            exchanges: u64::try_from(world.exchanges().len()).unwrap_or(u64::MAX),
        });
    }
    let trailers = writers.finish()?;

    let missing = match selection {
        Selection::Dev(list) => {
            let produced: std::collections::BTreeSet<&WorldKey> =
                worlds.iter().map(|entry| &entry.key).collect();
            list.worlds()
                .iter()
                .filter(|key| !produced.contains(key))
                .cloned()
                .collect()
        }
        Selection::Unsplit | Selection::Holdout { .. } => Vec::new(),
    };
    for world in &missing {
        tracing::warn!(dataset = %info.dataset, world = %world, "dev-listed world not produced by the source");
    }

    let mut settings = info.selection;
    settings.extend(selection.settings());
    let manifest = Manifest {
        format: FORMAT,
        dataset: info.dataset,
        dataset_version: info.dataset_version,
        split: selection.split(),
        source: info.source,
        converter: info.converter,
        selection: settings,
        pace: info.pace,
        worlds,
        files: FileDigests {
            messages: trailers.messages,
            exchanges: trailers.exchanges,
            labels: Some(trailers.labels),
        },
    };
    write_manifest(out_dir, &manifest)?;
    tracing::info!(
        dataset = %manifest.dataset,
        version = manifest.dataset_version,
        split = ?manifest.split,
        worlds = manifest.worlds.len(),
        failures = failures.len(),
        left_out,
        missing = missing.len(),
        out = %out_dir.display(),
        "export written"
    );
    Ok(Exported {
        manifest,
        failures,
        left_out,
        missing,
    })
}
