//! The reference matcher: the bench's "baseline detector 0", deliberately
//! naive span and shingle matching (ported from crosstalk-eval's
//! `reference`). It sets a floor any detector should beat and validates
//! labels: a construction-tier label it cannot find is worth a look. See
//! `docs/features/reference.md`.
//!
//! Per world, in exchange order, for each exchange of agent `A` (one agent
//! per client credential, [`agents`]):
//!
//! 1. **New inputs** are the request's messages beyond `A`'s previous
//!    request and response ([`delta`]). Each non-assistant part of them is
//!    scanned for other agents' indexed spans, then added to what `A` has
//!    seen.
//! 2. **Originated spans** are the runs of `A`'s response text (text,
//!    reasoning, tool-call arguments) whose k-gram shingles `A` has not seen
//!    in any input or earlier output, at least `min_span` folded bytes long.
//!    Their shingles are indexed, up to the boilerplate cutoff.
//!
//! Matching compares under [folding](text::fold); a hit is then
//! [classified](text::classify) `exact`, `normalized`, or `decoded` by one
//! string level, or counted out of reach when only two string levels
//! explain it. Encoded tokens (base64, hex, URL encoding) are decoded and
//! matched as `decoded`. Opaque blobs ([`text::opaque`]) are cut out before
//! spans, matching and decoding. Channel rereads are dropped, and hits
//! become one confirmed transmission per (reader exchange, sender, route)
//! whose quality is its strongest match ([`predict`]).

pub mod agents;
pub mod config;
pub mod delta;
pub mod error;
mod matcher;
pub mod output;
pub mod predict;
pub mod route;
pub mod text;

pub use config::{MAX_POSTINGS, ReferenceConfig};
pub use error::ReferenceError;
pub use matcher::run;
pub use output::{WorldOutput, WorldSummary};
