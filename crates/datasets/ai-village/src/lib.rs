//! AI Village (AI Digest): agents on frontier models living together in a
//! long-running village, with computers, a shared chat and memories.
//!
//! The dataset (`~/Data/ai/agents/ai-village`, gzipped JSON Lines; the
//! screenshot tars are never read) records model **responses** only:
//! `llm_calls` is withheld. Two sources come out of it:
//!
//! - [`Mode::ClaudeCode`] ([`claude_code`]): the one agent that ran the
//!   Claude Agent SDK, whose SDK entries give exact call boundaries and the
//!   tool results it read. The chat it read through the village MCP
//!   server's `get_events` tool is keyed by event id, so those labels are
//!   construction-tier. One world per context.
//! - [`Mode::Window`] ([`window`]): every standard agent over a window of
//!   village days, with requests rebuilt from the responses, structural
//!   chat labels and heuristic repository labels. One world per day.
//!
//! Which shared resources a bash command read or wrote is the bench's own
//! shell model ([`shell`]), not any detector's extractor. The normative
//! definition is `docs/features/dataset-ai-village.md`.

pub mod claude_code;
pub mod fold;
pub mod labels;
pub mod location;
pub mod provider;
pub mod rooms;
pub mod schema;
pub mod shell;
pub mod stream;
pub mod tables;
pub mod text;
pub mod time;
pub mod window;

use std::collections::BTreeMap;
use std::path::Path;

use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_format::ids::{DatasetId, InvalidKey};
use a2a_bench_format::labels::InvalidLabel;
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::InvalidMessage;
use serde::{Deserialize, Serialize};

use claude_code::{ClaudeCodeStats, ClaudeCodeStream};
use location::LocationError;
use stream::{StreamError, Table};
use time::{Day, TimeError};
use window::{WindowStats, WindowStream};

/// The dataset's id.
pub const DATASET: &str = "ai-village";

/// The converter's version: `ai-village@1` reproduces ct-eval's labels at
/// crosstalk 7f8a2fb.
pub const VERSION: u32 = 1;

/// The default window: Monday 2026-07-13 to Friday 2026-07-17 (26 agents,
/// about 116k turns).
pub const DEFAULT_FROM: &str = "2026-07-13";
pub const DEFAULT_TO: &str = "2026-07-17";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Stream(#[from] StreamError),
    #[error("time: {0}")]
    Time(#[from] TimeError),
    #[error("the dataset has no {0} agent")]
    NoAgent(String),
    #[error("location: {0}")]
    Location(#[from] LocationError),
    #[error("label: {0}")]
    Label(#[from] InvalidLabel),
    #[error("message: {0}")]
    Message(#[from] InvalidMessage),
    #[error("key: {0}")]
    Key(#[from] InvalidKey),
    #[error("corpus: {0}")]
    Corpus(Box<CorpusError>),
    #[error("--hours needs --from equal to --to (from {from}, to {to})")]
    HoursOverDays { from: Day, to: Day },
}

impl From<CorpusError> for Error {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}

/// Which part of the dataset to convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The Claude Code agent's stream, at most `limit` contexts.
    ClaudeCode { limit: Option<usize> },
    /// Village days `from..=to`.
    Window { from: Day, to: Day },
    /// The first `hours` hours of village day `day` (a bounded window for
    /// slow detectors).
    DaySlice { day: Day, hours: u32 },
}

/// Which part ct-eval's `--mode` selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModeName {
    #[default]
    Window,
    ClaudeCode,
}

impl ModeName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::ClaudeCode => "claude-code",
        }
    }
}

/// The selection flags ct-eval's CLI had for this dataset, with its
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// `--mode` (default `window`).
    pub mode: ModeName,
    /// `--from`, the first village day of a window.
    pub from: Day,
    /// `--to`, the last village day of a window, included.
    pub to: Day,
    /// `--hours`: only the first this many hours of the village day (needs
    /// `from == to`).
    pub hours: Option<u32>,
    /// `--limit`: at most this many Claude Code contexts.
    pub limit: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: ModeName::Window,
            from: Day {
                year: 2026,
                month: 7,
                day: 13,
            },
            to: Day {
                year: 2026,
                month: 7,
                day: 17,
            },
            hours: None,
            limit: None,
        }
    }
}

impl Options {
    /// The mode these flags select.
    pub fn resolve(&self) -> Result<Mode, Error> {
        Ok(match self.mode {
            ModeName::ClaudeCode => Mode::ClaudeCode { limit: self.limit },
            ModeName::Window => match self.hours {
                None => Mode::Window {
                    from: self.from,
                    to: self.to,
                },
                Some(hours) if self.from == self.to => Mode::DaySlice {
                    day: self.from,
                    hours,
                },
                Some(_) => {
                    return Err(Error::HoursOverDays {
                        from: self.from,
                        to: self.to,
                    });
                }
            },
        })
    }

    /// The manifest's `selection`: only the flags the mode reads.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let mut out = BTreeMap::new();
        out.insert(
            "mode".to_owned(),
            Setting::Text(self.mode.as_str().to_owned()),
        );
        match self.mode {
            ModeName::ClaudeCode => {
                if let Some(limit) = self.limit {
                    out.insert(
                        "limit".to_owned(),
                        Setting::Int(i64::try_from(limit).unwrap_or(i64::MAX)),
                    );
                }
            }
            ModeName::Window => {
                out.insert("from".to_owned(), Setting::Text(self.from.to_string()));
                out.insert("to".to_owned(), Setting::Text(self.to.to_string()));
                if let Some(hours) = self.hours {
                    out.insert("hours".to_owned(), Setting::Int(i64::from(hours)));
                }
            }
        }
        out
    }
}

/// What a source saw, for reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Stats {
    ClaudeCode(ClaudeCodeStats),
    Window(WindowStats),
}

enum Inner {
    ClaudeCode(Box<ClaudeCodeStream>),
    Window(Box<WindowStream>),
}

/// AI Village as a [`TraceSource`].
pub struct AiVillageSource {
    dataset: DatasetId,
    inner: Inner,
    files: FilesRead,
}

/// Opens the dataset at `root` with ct-eval's selection flags.
pub fn source(root: &Path, options: &Options) -> Result<AiVillageSource, Error> {
    AiVillageSource::open(root, options.resolve()?)
}

impl AiVillageSource {
    /// Opens the dataset at `root` and makes the passes `mode` needs.
    pub fn open(root: &Path, mode: Mode) -> Result<Self, Error> {
        let (inner, tables): (Inner, &[Table]) = match mode {
            Mode::ClaudeCode { limit } => (
                Inner::ClaudeCode(Box::new(ClaudeCodeStream::open(root, limit)?)),
                &ClaudeCodeStream::TABLES,
            ),
            Mode::Window { from, to } => (
                Inner::Window(Box::new(WindowStream::open(root, from, to)?)),
                &WindowStream::TABLES,
            ),
            Mode::DaySlice { day, hours } => (
                Inner::Window(Box::new(WindowStream::open_window(
                    root,
                    time::Window::first_hours(day, hours)?,
                )?)),
                &WindowStream::TABLES,
            ),
        };
        let mut files = FilesRead::new();
        for table in tables {
            let path = table.path(root);
            if path.exists() {
                files.record(path);
            }
        }
        Ok(Self {
            dataset: DatasetId::new(DATASET)?,
            inner,
            files,
        })
    }

    pub fn stats(&self) -> Stats {
        match &self.inner {
            Inner::ClaudeCode(stream) => Stats::ClaudeCode(stream.stats().clone()),
            Inner::Window(stream) => Stats::Window(stream.stats().clone()),
        }
    }

    /// The tables read, for the source digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }

    fn next_world(&mut self) -> Option<Result<World, Error>> {
        match &mut self.inner {
            Inner::ClaudeCode(stream) => stream.next_world(),
            Inner::Window(stream) => stream.next_world(),
        }
    }
}

impl TraceSource for AiVillageSource {
    type Error = Error;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, Error>> + '_ {
        std::iter::from_fn(move || self.next_world())
    }
}
