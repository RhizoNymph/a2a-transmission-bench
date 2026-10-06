//! Scoring for a2a-transmission-bench: a detector's predictions against an
//! export's labels. The alignment rule and judge decide every prediction
//! ([`score::align`], [`score::judge`]), the scorer counts them per row
//! ([`score::Scorer`]), the report and gates read the counts ([`report`]),
//! and [`run`] streams an export and a predictions file world by world. See
//! `docs/features/score.md`.

pub mod canon;
pub mod class;
pub mod notes;
pub mod predict;
pub mod report;
pub mod run;
pub mod score;
pub mod world;

pub use canon::{AsGiven, Canonicalize};
pub use world::World;
