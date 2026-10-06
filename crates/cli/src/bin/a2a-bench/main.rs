//! `a2a-bench`: parses the command line, runs the command, prints its
//! counts or table on stdout, logs to stderr (`RUST_LOG`, default `info`)
//! and picks the exit code:
//!
//! - 0: success;
//! - 1: an error, a failed validation, or an export with failed worlds;
//! - 2: `score`/`run` with a failed gate (ct-eval's); `diff` on an error;
//! - `diff`: 0 equal, 1 different, 2 error (as diff(1)).

use std::process::ExitCode;

use a2a_bench_cli::args::{Cli, Command};
use a2a_bench_cli::commands::{diff, export, input_view, run, score, validate};
use a2a_bench_score::report::table::render;
use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if let Err(error) = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_ansi(false)
        .try_init()
    {
        eprintln!("a2a-bench: logging is unavailable: {error}");
    }
    let cli = Cli::parse();
    let is_diff = matches!(cli.command, Command::Diff(_));
    match dispatch(cli.command) {
        Ok(code) => code,
        Err(error) => {
            tracing::error!(error = format!("{error:#}"), "a2a-bench failed");
            eprintln!("error: {error:#}");
            ExitCode::from(if is_diff { 2 } else { 1 })
        }
    }
}

fn dispatch(command: Command) -> Result<ExitCode> {
    match command {
        Command::Export(args) => {
            let outcome = export::export(&args).context("export")?;
            print!("{}", outcome.render());
            Ok(if outcome.summary.failed.is_empty() {
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "{} worlds failed to convert (see the log); the export holds the rest",
                    outcome.summary.failed.len()
                );
                ExitCode::FAILURE
            })
        }
        Command::Validate(args) => {
            let report = validate::validate(&args).context("validate")?;
            print!("{}", report.render());
            Ok(if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::InputView(args) => {
            let manifest = input_view::input_view(&args).context("input-view")?;
            print!("{}", input_view::render(&args, &manifest));
            Ok(ExitCode::SUCCESS)
        }
        Command::Run(args) => {
            let outcome = run::run(&args).context("run")?;
            print!("{}", render(&outcome.report));
            Ok(ExitCode::from(outcome.report.exit_code()))
        }
        Command::Score(args) => {
            let report = score::score(&score::ScoreRequest::from(&args)).context("score")?;
            print!("{}", render(&report));
            Ok(ExitCode::from(report.exit_code()))
        }
        Command::Diff(args) => {
            let tally = diff::diff(&args).context("diff")?;
            print!("{}", tally.render());
            Ok(if tally.equal() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}
