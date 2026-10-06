//! `a2a-bench input-view <export> <dest>`: the detector's input directory
//! (design §4): the manifest's input view, `messages.jsonl` and
//! `exchanges.jsonl`, hard-linked or copied. Never `labels.jsonl`.

use std::path::PathBuf;

use a2a_bench_corpus::export::{ExportError, input_view as build};
use a2a_bench_format::manifest::Manifest;
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct InputViewArgs {
    /// The export directory.
    pub export: PathBuf,
    /// The input directory to create (empty or new).
    pub dest: PathBuf,
}

/// Builds the input view; returns its manifest.
pub fn input_view(args: &InputViewArgs) -> Result<Manifest, ExportError> {
    build(&args.export, &args.dest)
}

/// Counts only.
pub fn render(args: &InputViewArgs, manifest: &Manifest) -> String {
    format!(
        "input view of {} ({} worlds) in {}\n",
        manifest.dataset,
        manifest.worlds.len(),
        args.dest.display()
    )
}
