//! The capture's `manifest.json`: the input view crosstalk's adapter wrote
//! and its predictions name (`Manifest::digest`). The bench only adds
//! truth to a capture, so the completed export's manifest must have this
//! manifest as its input view: same format, dataset, version, split,
//! source, converter, selection, pace, worlds (key, exchanges) and
//! message and exchange digests. The bench adds the labels digest and each
//! world's label count and notes, which the input view drops.

use std::path::Path;

use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::manifest::{Manifest, Setting};
use serde_json::Value;

use crate::VERSION;

/// The capture manifest's file name, beside its `messages.jsonl`.
pub const CAPTURE_MANIFEST_FILE: &str = "manifest.json";

/// Why a capture's manifest cannot be the export's input view.
#[derive(Debug, thiserror::Error)]
pub enum CaptureManifestError {
    #[error("reading it: {0}")]
    Read(String),
    #[error("it holds labels or notes: it is not an input view")]
    NotAnInputView,
    #[error("it is dataset {found}, the truth's scenario is {expected}")]
    Dataset {
        found: DatasetId,
        expected: DatasetId,
    },
    #[error("it is dataset version {found}, this converter writes {VERSION}")]
    Version { found: u32 },
    #[error("it does not hold exactly the truth's world")]
    Worlds,
    #[error("its selection is not the run window's margins the labels use")]
    Selection,
}

/// Reads the capture's manifest at `path`.
pub fn read(path: &Path) -> Result<Manifest, CaptureManifestError> {
    let bytes =
        std::fs::read(path).map_err(|error| CaptureManifestError::Read(error.to_string()))?;
    // serde's message may quote the file; keep only its position.
    serde_json::from_slice(&bytes).map_err(|error| {
        CaptureManifestError::Read(format!(
            "not a manifest (line {}, column {})",
            error.line(),
            error.column()
        ))
    })
}

/// Checks that `capture` is an input view of `dataset`'s one world `world`,
/// at this converter's version, cut with `selection`.
pub fn check(
    capture: &Manifest,
    dataset: &DatasetId,
    world: &WorldKey,
    selection: &std::collections::BTreeMap<String, Setting>,
) -> Result<(), CaptureManifestError> {
    if capture != &capture.input_view() {
        return Err(CaptureManifestError::NotAnInputView);
    }
    if &capture.dataset != dataset {
        return Err(CaptureManifestError::Dataset {
            found: capture.dataset.clone(),
            expected: dataset.clone(),
        });
    }
    if capture.dataset_version != VERSION {
        return Err(CaptureManifestError::Version {
            found: capture.dataset_version,
        });
    }
    if capture.worlds.len() != 1 || capture.worlds.iter().any(|entry| &entry.key != world) {
        return Err(CaptureManifestError::Worlds);
    }
    if &capture.selection != selection {
        return Err(CaptureManifestError::Selection);
    }
    Ok(())
}

/// The top-level manifest fields where `export`'s input view and
/// `capture` differ (values never shown).
pub fn differences(
    export: &Manifest,
    capture: &Manifest,
) -> Result<Vec<String>, serde_json::Error> {
    let view = serde_json::to_value(export.input_view())?;
    let capture = serde_json::to_value(capture)?;
    let (Value::Object(view), Value::Object(capture)) = (view, capture) else {
        return Ok(vec!["/".to_owned()]);
    };
    let mut keys: Vec<&String> = view.keys().chain(capture.keys()).collect();
    keys.sort();
    keys.dedup();
    Ok(keys
        .into_iter()
        .filter(|key| view.get(*key) != capture.get(*key))
        .map(|key| format!("/{key}"))
        .collect())
}
