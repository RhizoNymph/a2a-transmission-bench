//! LMCache agentic traces as a background (negative) corpus: ct-eval's
//! `lmcache` converter on the bench's types. See
//! `docs/features/dataset-lmcache.md`.
//!
//! Each row (`data/*.parquet`) is one request of one session: the
//! cumulative OpenAI-chat `input`, the `model`, and `pre_gap`, the seconds
//! since the session's previous request. A session's rows are contiguous
//! and in order (the converter relies on the Parquet row order). The
//! response to a request is not recorded; it is the assistant message the
//! session's next request appends, so the last request of every session has
//! no response and is dropped.
//!
//! - **Calls.** Request `r` sends `input_r` and receives the first assistant
//!   message of `input_{r+1}` past `input_r`'s length. `reconstructed` when
//!   `input_{r+1}` extends `input_r` unchanged, `synthetic` (with a debug
//!   line) when the history was rewritten in between.
//! - **Clock.** The dataset's own: a session starts at the corpus epoch;
//!   request `r` is at the sum of the `pre_gap`s up to it, in microseconds,
//!   kept strictly increasing. No pace.
//! - **Worlds.** Sessions are taken in turn from every row group of every
//!   selected file ([`Sessions`]), `agents_per_world` to a world. They never
//!   talked, so every world is a background world. Two sessions share a
//!   group (`shared_source`) when they solve the same SWE-bench
//!   repository's tasks or the same task otherwise.

pub mod calls;
pub mod error;
pub mod files;
pub mod options;
pub mod schema;
pub mod sessions;
pub mod source;

use std::path::Path;

pub use calls::{calls, group, mix, trajectory};
pub use error::LmcacheError;
pub use files::{Segment, discover, segments};
pub use options::Options;
pub use schema::LmcacheRow;
pub use sessions::{Session, Sessions};
pub use source::LmcacheSource;

/// The dataset's id (ct-eval's).
pub const DATASET: &str = "lmcache";

/// The converter's dataset version.
pub const VERSION: u32 = 1;

/// The trajectories mixed into one world unless configured (ct-eval's
/// `--agents-per-world` default, shared with open-swe).
pub const AGENTS_PER_WORLD: usize = 16;

/// The columns read.
pub const COLUMNS: &[&str] = &["session_id", "model", "input", "pre_gap"];

/// The source of `options` over the dataset at `root`. LMCache keeps its
/// recorded times, so it takes no pace.
pub fn source(root: &Path, options: &Options) -> Result<LmcacheSource, LmcacheError> {
    LmcacheSource::open(root, options)
}
