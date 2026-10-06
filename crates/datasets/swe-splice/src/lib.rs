//! Splices: exact channel transmissions planted in realistic SWE traces.
//! ct-eval's `swe_splice` generator on the bench's types; see
//! `docs/features/dataset-swe-splice.md`.
//!
//! Each world is two unrelated Open-SWE trajectories (different
//! repositories): a sender `A` and a reader `B`.
//!
//! 1. **The write.** A whole-file write of `A` ([`write`](mod@write)): an
//!    editor `create` or a `cat > P <<EOF` heredoc, at least
//!    [`MIN_CONTENT`] bytes over three lines or more.
//! 2. **One working directory.** `A`'s working directory (from its task's
//!    `<uploaded_files>`, else `/testbed`) is rewritten to `B`'s in every
//!    message of `A`, so the path `A` wrote is a path in `B`'s workspace.
//! 3. **The read.** Before one of `B`'s calls (never its first), `B` gets an
//!    extra call reading that path and its result in `B`'s harness format
//!    ([`read`]): the file as the [`variant`] renders it, numbered like
//!    `cat -n`.
//! 4. **The clock.** `A`'s call `i` is at `pace.at(a0 + i, 1, 0)` and
//!    `B`'s call `j` at `pace.at(b0 + j, 0, 0)`, with the offsets chosen so
//!    `B`'s read call comes right after `A`'s writing call, and the reader
//!    exchange (the call after the read) one step later still.
//!
//! **Label.** `A → B`, channel route through the shared absolute path (a
//! `file` resource), tool-result carrier, at `B`'s first exchange carrying
//! the result, located at the numbered lines, needing what the variant and
//! read form need, tier construction. The rest of the world is background:
//! complete coverage, and a negative control for every other (sender,
//! reader exchange).
//!
//! **Determinism.** Trajectories are read round-robin from the selected
//! shards into a pool; splice `n` draws its pair, write and insertion point
//! from a stream derived from the seed and `n`, and uses variant `n mod 4`.

pub mod error;
pub mod options;
pub mod path;
pub mod plan;
pub mod pool;
pub mod read;
pub mod source;
pub mod variant;
pub mod world;
pub mod write;

use std::path::Path;

use a2a_bench_corpus::clock::Pace;

pub use error::SpliceError;
pub use options::Options;
pub use plan::{Plan, plan};
pub use pool::{Pooled, insertion_points, rewrite_workdir, splicable, workdir};
pub use read::ReadForm;
pub use source::SpliceSource;
pub use variant::Variant;
pub use world::world;
pub use write::{FileWrite, WriteForm};

/// The dataset's id (ct-eval's).
pub const DATASET: &str = "swe_splice";

/// The converter's dataset version.
pub const VERSION: u32 = 1;

/// Splices per run unless configured.
pub const SPLICES: usize = 40;

/// The shortest file worth splicing, in bytes.
pub const MIN_CONTENT: usize = 160;

/// The longest file spliced, in bytes (a view of a huge file is cut).
pub const MAX_CONTENT: usize = 16_000;

/// The working directory when a task names none (SWE-agent and
/// mini-swe-agent mount the repository there).
pub const DEFAULT_WORKDIR: &str = "/testbed";

/// The source of `options` over Open-SWE-Traces at `root`, calls `pace`
/// apart. ct-eval drew the pace's steps from its corpus seed: for its
/// corpus, pass a pace whose seed is `options.seed`.
pub fn source(root: &Path, options: &Options, pace: Pace) -> Result<SpliceSource, SpliceError> {
    SpliceSource::open(root, options, pace)
}
