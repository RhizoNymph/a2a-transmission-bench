//! AgentDojo (prompt-injection attacks on tool-using agents): one agent per
//! run, attacked through injections planted in its tools' data. A port of
//! crosstalk-eval's converter at 7f8a2fb onto the bench's types; version 1
//! reproduces its corpus exactly. See `docs/features/dataset-agentdojo.md`.
//!
//! One run file (`runs/<pipeline>/<suite>/<task>/<attack>/<file>.json`) is
//! one world:
//!
//! - **victim**: the pipeline's model. Its exchange `k` is the prefix of the
//!   conversation up to assistant message `k`. Tool schemas are not recorded
//!   and are left out.
//! - **attacker** (attacked runs only): a synthetic agent standing for the
//!   injections' author, with one `Synthetic` exchange before the victim's
//!   first whose response writes every injection.
//!
//! Labels are in [`truth`]; how each injection arrived is in [`classify`].
//! Coverage is complete at the construction tier. Runs without an attack
//! (`none`), including the `injection_task_*` runs whose user prompt is the
//! attacker's goal, have no attacker and no labels.

pub mod classify;
mod convert;
mod error;
pub mod files;
mod location;
pub mod messages;
mod options;
pub mod route;
pub mod schema;
mod source;
pub mod tally;
pub mod truth;

pub use convert::{Loaded, convert_run, load_world, model_of};
pub use error::AgentDojoError;
pub use options::Options;
pub use source::{AgentDojoSource, source};
pub use tally::Tally;

/// ct-eval's dataset id.
pub const DATASET: &str = "agentdojo";
/// The dataset version (`agentdojo@1`): ct-eval's corpus at 7f8a2fb.
pub const VERSION: u32 = 1;
/// The victim agent's name in every world.
pub const VICTIM: &str = "victim";
/// The synthetic attacker's name in attacked worlds.
pub const ATTACKER: &str = "attacker";
/// The manifest note counting controls left out because no exchange of the
/// world carries their prompt.
pub const UNCARRIED_CONTROL: &str = "uncarried_control";
