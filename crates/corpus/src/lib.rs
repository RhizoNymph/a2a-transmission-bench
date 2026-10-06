//! The corpus model of a2a-transmission-bench: how a converter turns a
//! dataset into worlds ([`world::WorldBuilder`], the virtual clock
//! [`clock::Pace`]), streams them ([`source::TraceSource`]), and writes an
//! export ([`export::export`]) of the selected split ([`split::Selection`]).
//! See `docs/features/corpus.md`.

pub mod clock;
pub mod config;
pub mod delta;
pub mod export;
pub mod helpers;
pub mod source;
pub mod split;
pub mod world;
