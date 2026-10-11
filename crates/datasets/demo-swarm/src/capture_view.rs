//! The capture's manifest as an export records it, when the export's input
//! view is not the capture's: a holdout export (`split`, `release`), or an
//! export at a newer dataset version than the capture's (labels changed,
//! the capture's messages and exchanges did not).
//!
//! crosstalk's adapter writes the capture with the dataset version it
//! knows, and its predictions name that manifest's digest. A label fix
//! bumps only the bench converter's version, so the adapter and its
//! captures need not change: the export records, in its `selection`,
//!
//! - [`CAPTURE_DIGEST_KEY`]: the capture manifest's digest
//!   (`Manifest::digest`, hex), and
//! - [`CAPTURE_VERSION_KEY`]: the capture's dataset version, only when it
//!   is older than the export's,
//!
//! and the scorer accepts predictions naming the recorded digest. It is
//! never trusted as written: [`capture_digest`] rebuilds the capture view
//! from the export ([`capture_view`]: split, release, version and these
//! keys undone) and requires it to digest to the recorded value.

use a2a_bench_corpus::split::RELEASE_KEY;
use a2a_bench_format::ids::Digest;
use a2a_bench_format::manifest::{Manifest, Setting, Split};

use crate::DATASET_PREFIX;

/// The `selection` key of the capture manifest's digest.
pub const CAPTURE_DIGEST_KEY: &str = "capture_digest";

/// The `selection` key of the capture's dataset version, when it is older
/// than the export's.
pub const CAPTURE_VERSION_KEY: &str = "capture_dataset_version";

/// Why a capture view cannot be recorded, or read back.
#[derive(Debug, thiserror::Error)]
pub enum CaptureViewError {
    #[error("digesting a manifest: {0}")]
    Digest(#[source] serde_json::Error),
    #[error("the demo-swarm export records no {CAPTURE_DIGEST_KEY}")]
    Missing,
    #[error("the demo-swarm export's {0} is malformed")]
    Malformed(&'static str),
    #[error(
        "the demo-swarm export's {CAPTURE_VERSION_KEY} is {recorded}, not a version older than the export's {export}"
    )]
    Version { recorded: i64, export: u32 },
    #[error(
        "the demo-swarm export's {CAPTURE_DIGEST_KEY} is {recorded}, its capture view digests to {actual}"
    )]
    Mismatch { recorded: Digest, actual: Digest },
}

/// Records `capture`'s digest, and its dataset version when it differs
/// from `manifest`'s, in `manifest`'s selection.
pub fn record(manifest: &mut Manifest, capture: &Manifest) -> Result<(), CaptureViewError> {
    let digest = capture.digest().map_err(CaptureViewError::Digest)?;
    manifest.selection.insert(
        CAPTURE_DIGEST_KEY.to_owned(),
        Setting::Text(digest.to_hex()),
    );
    if capture.dataset_version != manifest.dataset_version {
        manifest.selection.insert(
            CAPTURE_VERSION_KEY.to_owned(),
            Setting::Int(i64::from(capture.dataset_version)),
        );
    }
    Ok(())
}

fn is_demo_swarm(manifest: &Manifest) -> bool {
    manifest
        .dataset
        .as_str()
        .strip_prefix(DATASET_PREFIX)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The capture's manifest as `export` implies it: the export's input view
/// with `split: dev` and no release (a holdout), the recorded capture
/// version (if any), and without the capture keys.
pub fn capture_view(export: &Manifest) -> Result<Manifest, CaptureViewError> {
    let mut view = export.input_view();
    if view.split == Split::Holdout {
        view.split = Split::Dev;
        view.selection.remove(RELEASE_KEY);
    }
    view.selection.remove(CAPTURE_DIGEST_KEY);
    if let Some(recorded) = view.selection.remove(CAPTURE_VERSION_KEY) {
        let Setting::Int(recorded) = recorded else {
            return Err(CaptureViewError::Malformed(CAPTURE_VERSION_KEY));
        };
        view.dataset_version = u32::try_from(recorded)
            .ok()
            .filter(|version| (1..export.dataset_version).contains(version))
            .ok_or(CaptureViewError::Version {
                recorded,
                export: export.dataset_version,
            })?;
    }
    Ok(view)
}

/// The capture digest a demo-swarm export's predictions may name instead
/// of its own: `None` for an export of another dataset, or a dev export
/// that records none (its input view is the capture's). A holdout export
/// must record one. The recorded [`CAPTURE_DIGEST_KEY`] must equal the
/// digest of [`capture_view`].
pub fn capture_digest(export: &Manifest) -> Result<Option<Digest>, CaptureViewError> {
    if !is_demo_swarm(export) {
        return Ok(None);
    }
    let recorded: Digest = match export.selection.get(CAPTURE_DIGEST_KEY) {
        Some(Setting::Text(text)) => text
            .parse()
            .map_err(|_| CaptureViewError::Malformed(CAPTURE_DIGEST_KEY))?,
        Some(_) => return Err(CaptureViewError::Malformed(CAPTURE_DIGEST_KEY)),
        None if export.split == Split::Holdout
            || export.selection.contains_key(CAPTURE_VERSION_KEY) =>
        {
            return Err(CaptureViewError::Missing);
        }
        None => return Ok(None),
    };
    let actual = capture_view(export)?
        .digest()
        .map_err(CaptureViewError::Digest)?;
    if recorded == actual {
        Ok(Some(recorded))
    } else {
        Err(CaptureViewError::Mismatch { recorded, actual })
    }
}
