//! A demo-swarm holdout export (design §9.2): a whole run, made with a
//! holdout seed, exported for one release.
//!
//! The capture crosstalk's adapter writes is a dev input view, and its
//! predictions name that view's digest. A holdout export's manifest is the
//! capture's with exactly three changes, all made here:
//!
//! - `split`: `dev` → `holdout`;
//! - `selection.release`: the release (`<detector>@<tag>`), as every
//!   holdout export records it ([`RELEASE_KEY`]);
//! - `selection.capture_digest`: the capture manifest's digest
//!   (`Manifest::digest`, hex), [`CAPTURE_DIGEST_KEY`].
//!
//! So the export's input view has its own digest (what a detector run on
//! it names), and the capture's predictions are recognised through
//! [`capture_digest`]: the recorded digest, checked against the capture
//! view rebuilt from the export ([`capture_view`]: those three changes
//! undone), so it cannot name any other manifest.

use a2a_bench_corpus::split::{RELEASE_KEY, Release};
use a2a_bench_format::ids::Digest;
use a2a_bench_format::manifest::{Manifest, Setting, Split};

use crate::DATASET_PREFIX;

/// The `selection` key of a holdout export's capture manifest digest.
pub const CAPTURE_DIGEST_KEY: &str = "capture_digest";

/// Why a manifest cannot be made, or read, as a demo-swarm holdout.
#[derive(Debug, thiserror::Error)]
pub enum HoldoutError {
    #[error("the capture's manifest is split {0:?}, a holdout is made from a dev capture")]
    CaptureNotDev(Split),
    #[error("the capture's selection already holds {0:?}")]
    Reserved(&'static str),
    #[error("digesting a manifest: {0}")]
    Digest(#[source] serde_json::Error),
    #[error("the demo-swarm holdout export records no {CAPTURE_DIGEST_KEY}")]
    Missing,
    #[error("the demo-swarm holdout export's {CAPTURE_DIGEST_KEY} is not a digest")]
    Malformed,
    #[error(
        "the demo-swarm holdout export's {CAPTURE_DIGEST_KEY} is {recorded}, its capture view digests to {actual}"
    )]
    Mismatch { recorded: Digest, actual: Digest },
}

/// A holdout is made only from a dev capture whose selection leaves the
/// holdout's keys free.
pub fn check_capture(capture: &Manifest) -> Result<(), HoldoutError> {
    if capture.split != Split::Dev {
        return Err(HoldoutError::CaptureNotDev(capture.split));
    }
    for key in [RELEASE_KEY, CAPTURE_DIGEST_KEY] {
        if capture.selection.contains_key(key) {
            return Err(HoldoutError::Reserved(key));
        }
    }
    Ok(())
}

/// Turns `manifest` (equal to `capture` but for labels) into the holdout
/// export's manifest for `release` (module docs).
pub fn mark(
    manifest: &mut Manifest,
    capture: &Manifest,
    release: &Release,
) -> Result<(), HoldoutError> {
    check_capture(capture)?;
    let digest = capture.digest().map_err(HoldoutError::Digest)?;
    manifest.split = Split::Holdout;
    manifest
        .selection
        .insert(RELEASE_KEY.to_owned(), Setting::Text(release.to_string()));
    manifest.selection.insert(
        CAPTURE_DIGEST_KEY.to_owned(),
        Setting::Text(digest.to_hex()),
    );
    Ok(())
}

/// Whether `manifest` is a demo-swarm holdout export.
pub fn is_holdout(manifest: &Manifest) -> bool {
    manifest.split == Split::Holdout
        && manifest
            .dataset
            .as_str()
            .strip_prefix(DATASET_PREFIX)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The capture's manifest as a holdout export implies it: the export's
/// input view with `split: dev` and without the holdout's selection keys.
pub fn capture_view(export: &Manifest) -> Manifest {
    let mut view = export.input_view();
    view.split = Split::Dev;
    view.selection.remove(RELEASE_KEY);
    view.selection.remove(CAPTURE_DIGEST_KEY);
    view
}

/// The capture digest a demo-swarm holdout export's predictions may name
/// instead of its own: `None` for any other manifest. The recorded
/// [`CAPTURE_DIGEST_KEY`] must be present and equal the digest of
/// [`capture_view`].
pub fn capture_digest(export: &Manifest) -> Result<Option<Digest>, HoldoutError> {
    if !is_holdout(export) {
        return Ok(None);
    }
    let recorded: Digest = match export.selection.get(CAPTURE_DIGEST_KEY) {
        Some(Setting::Text(text)) => text.parse().map_err(|_| HoldoutError::Malformed)?,
        Some(_) => return Err(HoldoutError::Malformed),
        None => return Err(HoldoutError::Missing),
    };
    let actual = capture_view(export)
        .digest()
        .map_err(HoldoutError::Digest)?;
    if recorded == actual {
        Ok(Some(recorded))
    } else {
        Err(HoldoutError::Mismatch { recorded, actual })
    }
}
