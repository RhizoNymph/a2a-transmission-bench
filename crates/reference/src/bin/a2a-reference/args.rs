//! Command-line arguments: the detector contract's two, and the matcher's
//! one flag.

use std::path::PathBuf;

use a2a_bench_reference::{MAX_POSTINGS, ReferenceConfig};
use clap::Parser;

/// The reference matcher: naive span and shingle matching over an export's
/// input view.
#[derive(Debug, Parser)]
#[command(name = "a2a-reference", version)]
pub struct Args {
    /// The input view: a directory holding manifest.json, messages.jsonl
    /// and exchanges.jsonl.
    #[arg(long)]
    pub input: PathBuf,
    /// Where to write predictions.jsonl.
    #[arg(long)]
    pub output: PathBuf,
    /// The boilerplate cutoff: the most originated spans a shingle may be
    /// posted for before it is ignored.
    #[arg(long, default_value_t = MAX_POSTINGS)]
    pub max_postings: usize,
}

impl Args {
    pub fn config(&self) -> ReferenceConfig {
        ReferenceConfig {
            max_postings: self.max_postings,
            ..ReferenceConfig::default()
        }
    }
}
