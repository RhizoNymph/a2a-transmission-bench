//! The `a2a-bench` command line as a library: every command is a function
//! from typed arguments to a typed outcome or a typed error, so the binary
//! (`src/bin/a2a-bench`) only parses, prints and picks the exit code. See
//! `docs/features/cli.md`.
//!
//! - [`commands::export`]: a dataset to an export directory (dev or
//!   holdout), with revision pinning and the holdout commitment.
//! - [`commands::validate`]: every format check over an export (and a
//!   predictions file), counts only.
//! - [`commands::input_view`]: the detector's input directory.
//! - [`commands::run`]: input view, detector process, score.
//! - [`commands::score`]: predictions against an export, with the bench's
//!   resource canonicaliser and the gates.
//! - [`commands::diff`]: two exports or two predictions files, row by row.
//!
//! Nothing here prints label or message text: outputs are counts and ids.

pub mod args;
pub mod canon;
pub mod commands;
pub mod datasets;
pub mod holdout;
pub mod repo;
pub mod revision;
pub mod safe;
