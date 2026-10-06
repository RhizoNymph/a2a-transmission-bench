//! The command line: one subcommand per command module.

use clap::{Parser, Subcommand};

use crate::commands::diff::DiffArgs;
use crate::commands::export::ExportArgs;
use crate::commands::input_view::InputViewArgs;
use crate::commands::run::RunArgs;
use crate::commands::score::ScoreArgs;
use crate::commands::validate::ValidateArgs;

/// a2a-transmission-bench: export datasets, run detectors, score them.
#[derive(Debug, Parser)]
#[command(name = "a2a-bench", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Convert a dataset into an export directory (dev by default).
    Export(Box<ExportArgs>),
    /// Run every format check over an export (and a predictions file).
    Validate(ValidateArgs),
    /// Build a detector's input directory from an export (never the labels).
    InputView(InputViewArgs),
    /// Build the input view, run a detector over it, score its predictions.
    Run(RunArgs),
    /// Score a predictions file against an export; exit 2 on a failed gate.
    Score(ScoreArgs),
    /// Compare two exports or two predictions files row by row.
    Diff(DiffArgs),
}
