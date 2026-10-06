//! Helpers shared by converters: OpenAI-chat messages, background worlds of
//! independent agents, media kinds, a seeded generator for synthetic
//! corpora and, with the `parquet` feature, typed Parquet rows.

pub mod background;
pub mod chat;
pub mod media;
#[cfg(feature = "parquet")]
pub mod parquet_rows;
pub mod rng;
