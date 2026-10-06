//! τ²-bench (customer-service agents and an LLM user simulator): two model
//! agents per simulation, talking turn by turn. A port of crosstalk-eval's
//! converter at 7f8a2fb onto the bench's types; version 1 reproduces its
//! corpus exactly. See `docs/features/dataset-tau2.md`.
//!
//! One simulation of a results file is one world:
//!
//! - **agent**: the agent model. Its view ([`views`]) is its reconstructed
//!   system prompt ([`prompts`]), the conversation, and its own tools.
//! - **user** (absent when the agent works alone, `dummy_user`): the user
//!   simulator, seeing the conversation with roles flipped and its own
//!   tools.
//!
//! Every assistant or user record with `raw_data` is one model call, an
//! exchange at its recorded time; the agent's hard-coded greeting is not.
//! Labels are in [`truth`]. There are no attacks: this is a benign baseline
//! for precision. Coverage is complete at the structural tier.

mod convert;
mod error;
pub mod files;
mod options;
pub mod prompts;
pub mod schema;
mod source;
pub mod time;
pub mod truth;
pub mod views;

pub use convert::{convert_simulation, load_results};
pub use error::Tau2Error;
pub use options::Options;
pub use source::{Tau2Source, source};

/// ct-eval's dataset id.
pub const DATASET: &str = "tau2";
/// The dataset version (`tau2@1`): ct-eval's corpus at 7f8a2fb.
pub const VERSION: u32 = 1;
/// The agent's name in every world.
pub const AGENT: &str = "agent";
/// The user simulator's name in every world that has one.
pub const USER: &str = "user";
