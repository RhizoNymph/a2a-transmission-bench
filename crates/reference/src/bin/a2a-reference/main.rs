//! `a2a-reference`: the reference matcher as a bench detector.
//!
//! ```text
//! a2a-reference --input <dir> --output <predictions.jsonl> [--max-postings N]
//! ```
//!
//! Reads the input view in `<dir>` (`manifest.json`, `messages.jsonl`,
//! `exchanges.jsonl`) world by world and writes one predictions file. A
//! world it cannot process is a `failed` world row; anything that makes the
//! files unreadable or inconsistent is a run failure (exit 1, no trailer).
//! Logs go to stderr (`RUST_LOG`, default `info`).

mod args;
mod detect;
mod inputs;

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if let Err(error) = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init()
    {
        eprintln!("a2a-reference: logging is unavailable: {error}");
    }
    let args = args::Args::parse();
    match detect::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = format!("{error:#}"), "run failed");
            ExitCode::FAILURE
        }
    }
}
