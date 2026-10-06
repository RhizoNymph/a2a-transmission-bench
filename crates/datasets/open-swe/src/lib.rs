//! Open-SWE-Traces as a background (negative) corpus: ct-eval's `open_swe`
//! converter on the bench's types. See `docs/features/dataset-open-swe.md`.
//!
//! Each row of a shard (`data/<harness>/<model>/<dataset>/*.parquet`) is
//! one trajectory: one agent solving one SWE task alone. Its calls are its
//! assistant messages: call `i` sends `messages[..i]` and receives
//! `messages[i]`. Tool messages carry no `tool_call_id`, so they are paired
//! with calls by position (`a2a_bench_corpus::helpers::chat`).
//!
//! **Worlds.** Trajectories are mixed into worlds of `agents_per_world`,
//! one row from each selected shard in turn ([`RoundRobin`]), so a world
//! holds several harnesses, models and repositories. No trajectory read
//! another, so every world is a background world: complete coverage, no
//! positives, a `shared_source` control between trajectories of one
//! repository and a `boilerplate` control otherwise.
//!
//! **Clock.** Traces have no times. Trajectory slot `t` of a world makes its
//! call `i` at `pace.at(i, t, 0)`: every trajectory starts together and
//! they interleave call by call.

pub mod error;
pub mod files;
pub mod options;
pub mod rows;
pub mod schema;
pub mod source;
pub mod trajectory;

use std::path::Path;

use a2a_bench_corpus::clock::Pace;

pub use error::OpenSweError;
pub use files::{Shard, discover};
pub use options::Options;
pub use rows::RoundRobin;
pub use schema::OpenSweRow;
pub use source::OpenSweSource;
pub use trajectory::{agent_name, calls, mix, trajectory};

/// The dataset's id (ct-eval's).
pub const DATASET: &str = "open_swe";

/// The converter's dataset version.
pub const VERSION: u32 = 1;

/// The trajectories mixed into one world unless configured.
pub const AGENTS_PER_WORLD: usize = 16;

/// The columns read.
pub const COLUMNS: &[&str] = &["instance_id", "repo", "trajectory_id", "messages"];

/// The source of `options` over the dataset at `root`, calls `pace` apart.
pub fn source(root: &Path, options: &Options, pace: Pace) -> Result<OpenSweSource, OpenSweError> {
    OpenSweSource::open(root, options, pace)
}
