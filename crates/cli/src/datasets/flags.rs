//! The dataset flags of `a2a-bench export`: ct-eval's `run`/`truth` source
//! flags, with its defaults, plus demo-swarm's. A flag the chosen dataset
//! does not read is refused rather than ignored.

use std::path::PathBuf;
use std::time::Duration;

use a2a_bench_corpus::clock::{ClockError, Pace};
use a2a_bench_dataset_ai_village::time::{Day, TimeError};
use a2a_bench_dataset_ai_village::{self as ai_village, ModeName};
use clap::{Args, ValueEnum};

use super::DatasetName;

/// ct-eval's default pace bounds (`--pace-min-ms`, `--pace-max-ms`).
pub const DEFAULT_PACE_MIN_MS: u64 = 1_000;
pub const DEFAULT_PACE_MAX_MS: u64 = 5_000;

/// AI Village: which part to convert (ct-eval's `--mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VillageMode {
    /// Every agent over `--from`..=`--to`, one world per village day.
    Window,
    /// The Claude Code agent's stream, one world per context.
    ClaudeCode,
}

/// Selection flags; each dataset reads some of them (see
/// `docs/features/cli.md`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct DatasetFlags {
    /// At most this many worlds or files (SALT: trace files, stratified
    /// across conditions; wiki: worlds, largest first; open-swe, lmcache:
    /// shards; cipher: pools; AI Village: Claude Code contexts).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Keep only files whose path contains this (repeatable; AgentDojo:
    /// `pipeline=…`, `suite=…`, `attack=…`, `task=…`).
    #[arg(long)]
    pub include: Vec<String>,
    /// Trajectories mixed into one world (open-swe, lmcache; default 16).
    #[arg(long)]
    pub agents_per_world: Option<usize>,
    /// Rows (open-swe) or sessions (lmcache) per file, splices (swe-splice,
    /// default 40) or pairs per cipher (cipher, default 24).
    #[arg(long)]
    pub count: Option<usize>,
    /// Seeds the synthetic corpora (swe-splice, cipher) and the pace clock
    /// (default 0).
    #[arg(long)]
    pub corpus_seed: Option<u64>,
    /// The shortest step between two calls, in ms, for datasets without
    /// times (default 1000).
    #[arg(long)]
    pub pace_min_ms: Option<u64>,
    /// The longest such step, in ms (default 5000).
    #[arg(long)]
    pub pace_max_ms: Option<u64>,
    /// AI Village: which part to convert (default window).
    #[arg(long, value_enum)]
    pub mode: Option<VillageMode>,
    /// AI Village window: the first village day, YYYY-MM-DD (default
    /// 2026-07-13).
    #[arg(long)]
    pub from: Option<String>,
    /// AI Village window: the last village day, included (default
    /// 2026-07-17).
    #[arg(long)]
    pub to: Option<String>,
    /// AI Village window: only the first this many hours of the day; needs
    /// --from equal to --to.
    #[arg(long)]
    pub hours: Option<u32>,
    /// Wiki: keep only pages in these task clusters (repeatable).
    #[arg(long)]
    pub family: Vec<String>,
    /// Wiki: keep only pages on these wikis (repeatable).
    #[arg(long)]
    pub wiki: Vec<String>,
    /// Wiki: keep only worlds with at least this many agents.
    #[arg(long)]
    pub min_agents: Option<usize>,
    /// Wiki: drop worlds with more than this many agents.
    #[arg(long)]
    pub max_agents: Option<usize>,
    /// Wiki: the demo subset; overrides the other wiki filters and --limit.
    #[arg(long)]
    pub demo: bool,
    /// demo-swarm: the bench input dir `ct-bench-detect from-export` wrote
    /// (messages.jsonl, exchanges.jsonl).
    #[arg(long)]
    pub inputs: Option<PathBuf>,
    /// demo-swarm: the swarm's truth.jsonl.
    #[arg(long)]
    pub truth: Option<PathBuf>,
    /// demo-swarm: run window margin before the truth's start, in ms.
    #[arg(long)]
    pub run_lead_ms: Option<u64>,
    /// demo-swarm: run window margin after the truth's last row, in ms.
    #[arg(long)]
    pub run_slack_ms: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum FlagError {
    #[error("--{flag} does not apply to {dataset}")]
    NotRead {
        dataset: &'static str,
        flag: &'static str,
    },
    #[error("{dataset} needs --{flag}")]
    Missing {
        dataset: &'static str,
        flag: &'static str,
    },
    #[error("--pace-min-ms and --pace-max-ms: {0}")]
    Pace(#[from] ClockError),
    #[error("--from/--to: {0}")]
    Day(#[from] TimeError),
}

const PACE: [&str; 3] = ["corpus-seed", "pace-min-ms", "pace-max-ms"];

impl DatasetFlags {
    /// The flags given, by name.
    fn given(&self) -> Vec<&'static str> {
        let mut given = Vec::new();
        let mut flag = |set: bool, name: &'static str| {
            if set {
                given.push(name);
            }
        };
        flag(self.limit.is_some(), "limit");
        flag(!self.include.is_empty(), "include");
        flag(self.agents_per_world.is_some(), "agents-per-world");
        flag(self.count.is_some(), "count");
        flag(self.corpus_seed.is_some(), "corpus-seed");
        flag(self.pace_min_ms.is_some(), "pace-min-ms");
        flag(self.pace_max_ms.is_some(), "pace-max-ms");
        flag(self.mode.is_some(), "mode");
        flag(self.from.is_some(), "from");
        flag(self.to.is_some(), "to");
        flag(self.hours.is_some(), "hours");
        flag(!self.family.is_empty(), "family");
        flag(!self.wiki.is_empty(), "wiki");
        flag(self.min_agents.is_some(), "min-agents");
        flag(self.max_agents.is_some(), "max-agents");
        flag(self.demo, "demo");
        flag(self.inputs.is_some(), "inputs");
        flag(self.truth.is_some(), "truth");
        flag(self.run_lead_ms.is_some(), "run-lead-ms");
        flag(self.run_slack_ms.is_some(), "run-slack-ms");
        given
    }

    /// Refuses a flag `dataset` does not read.
    pub fn check(&self, dataset: DatasetName) -> Result<(), FlagError> {
        let reads: &[&str] = match dataset {
            DatasetName::Salt | DatasetName::Agentdojo => &["limit", "include"],
            DatasetName::Tau2 => &["limit", "include"],
            DatasetName::CollusionWiki => &[
                "limit",
                "family",
                "wiki",
                "min-agents",
                "max-agents",
                "demo",
            ],
            DatasetName::SwarmTraces => &["limit"],
            DatasetName::OpenSwe | DatasetName::Lmcache => {
                &["limit", "include", "count", "agents-per-world"]
            }
            DatasetName::SweSplice | DatasetName::Cipher => &["limit", "include", "count"],
            DatasetName::AiVillage => &["mode", "from", "to", "hours", "limit"],
            DatasetName::DemoSwarm => &["inputs", "truth", "run-lead-ms", "run-slack-ms"],
        };
        for flag in self.given() {
            let paced = dataset.paced() && PACE.contains(&flag);
            if !paced && !reads.contains(&flag) {
                return Err(FlagError::NotRead {
                    dataset: dataset.id(),
                    flag,
                });
            }
        }
        Ok(())
    }

    /// The corpus seed (default 0).
    pub fn seed(&self) -> u64 {
        self.corpus_seed.unwrap_or(0)
    }

    /// ct-eval's pace: `Pace::new(min, max, seed)`, 1 s to 5 s and seed 0 by
    /// default.
    pub fn pace(&self) -> Result<Pace, FlagError> {
        Ok(Pace::new(
            Duration::from_millis(self.pace_min_ms.unwrap_or(DEFAULT_PACE_MIN_MS)),
            Duration::from_millis(self.pace_max_ms.unwrap_or(DEFAULT_PACE_MAX_MS)),
            self.seed(),
        )?)
    }

    /// AI Village's options (ct-eval's defaults).
    pub fn ai_village(&self) -> Result<ai_village::Options, FlagError> {
        let mut options = ai_village::Options::default();
        if let Some(mode) = self.mode {
            options.mode = match mode {
                VillageMode::Window => ModeName::Window,
                VillageMode::ClaudeCode => ModeName::ClaudeCode,
            };
        }
        if let Some(from) = &self.from {
            options.from = Day::parse(from)?;
        }
        if let Some(to) = &self.to {
            options.to = Day::parse(to)?;
        }
        options.hours = self.hours;
        options.limit = self.limit;
        Ok(options)
    }
}
