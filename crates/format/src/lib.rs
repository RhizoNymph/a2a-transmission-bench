//! The on-disk format of a2a-transmission-bench (`a2a-bench/1`): messages,
//! exchanges, labels and predictions as framed JSONL, and the manifest. See
//! `docs/features/format.md`.

pub mod check;
pub mod exchange;
pub mod files;
pub mod ids;
pub mod json;
pub mod jsonl;
pub mod labels;
pub mod location;
pub mod manifest;
pub mod message;
pub mod predictions;
pub mod resource;
pub mod time;
pub mod version;
