//! A demo-swarm holdout export (design §9.2): a whole run, made with a
//! holdout seed, exported for one release.
//!
//! The capture crosstalk's adapter writes is a dev input view, and its
//! predictions name that view's digest. A holdout export's manifest is the
//! capture's with these changes:
//!
//! - `split`: `dev` → `holdout` ([`mark`]);
//! - `selection.release`: the release (`<detector>@<tag>`), as every
//!   holdout export records it ([`RELEASE_KEY`]) ([`mark`]);
//! - `selection.capture_digest` (and `selection.capture_dataset_version`
//!   for an older capture): recorded by [`crate::capture_view::record`],
//!   as for any export whose input view is not the capture's.
//!
//! So the export's input view has its own digest (what a detector run on
//! it names), and the capture's predictions are recognised through
//! [`crate::capture_view::capture_digest`].

use a2a_bench_corpus::split::{RELEASE_KEY, Release};
use a2a_bench_format::manifest::{Manifest, Setting, Split};

/// Why a manifest cannot be made a demo-swarm holdout.
#[derive(Debug, thiserror::Error)]
pub enum HoldoutError {
    #[error("the capture's manifest is split {0:?}, a holdout is made from a dev capture")]
    CaptureNotDev(Split),
    #[error("the capture's selection already holds {0:?}")]
    Reserved(&'static str),
}

/// A holdout is made only from a dev capture whose selection leaves the
/// release key free.
pub fn check_capture(capture: &Manifest) -> Result<(), HoldoutError> {
    if capture.split != Split::Dev {
        return Err(HoldoutError::CaptureNotDev(capture.split));
    }
    if capture.selection.contains_key(RELEASE_KEY) {
        return Err(HoldoutError::Reserved(RELEASE_KEY));
    }
    Ok(())
}

/// Marks `manifest` (the capture's but for labels and version) as the
/// holdout export's for `release`: split and release (module docs).
pub fn mark(
    manifest: &mut Manifest,
    capture: &Manifest,
    release: &Release,
) -> Result<(), HoldoutError> {
    check_capture(capture)?;
    manifest.split = Split::Holdout;
    manifest
        .selection
        .insert(RELEASE_KEY.to_owned(), Setting::Text(release.to_string()));
    Ok(())
}
