//! The datasets `a2a-bench export` knows, their flags (ct-eval's, with its
//! defaults), and the dispatch to each converter crate's `source(...)`.

mod dispatch;
mod flags;

use clap::ValueEnum;

pub use dispatch::{DatasetError, ExportRequest, ExportSummary, export, export_demo_swarm};
pub use flags::{DatasetFlags, FlagError, VillageMode};

/// A dataset the CLI exports. Values are the bench's dataset ids; ct-eval's
/// `--dataset` spellings (`wiki`, `swarm`, `open-swe`, `swe-splice`) are
/// accepted as aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ValueEnum)]
pub enum DatasetName {
    Salt,
    Agentdojo,
    Tau2,
    #[value(name = "collusion-wiki", alias = "wiki")]
    CollusionWiki,
    #[value(name = "swarm-traces", alias = "swarm")]
    SwarmTraces,
    #[value(name = "open_swe", alias = "open-swe")]
    OpenSwe,
    Lmcache,
    #[value(name = "swe_splice", alias = "swe-splice")]
    SweSplice,
    Cipher,
    #[value(name = "ai-village")]
    AiVillage,
    /// demo-swarm/headline or demo-swarm/boilerplate, by the truth header.
    #[value(name = "demo-swarm")]
    DemoSwarm,
}

impl DatasetName {
    /// The dataset id (the `datasets.toml` key; for demo-swarm the prefix of
    /// both ids).
    pub fn id(self) -> &'static str {
        match self {
            Self::Salt => a2a_bench_dataset_salt::DATASET,
            Self::Agentdojo => a2a_bench_dataset_agentdojo::DATASET,
            Self::Tau2 => a2a_bench_dataset_tau2::DATASET,
            Self::CollusionWiki => a2a_bench_dataset_wiki::DATASET,
            Self::SwarmTraces => a2a_bench_dataset_swarm::DATASET,
            Self::OpenSwe => a2a_bench_dataset_open_swe::DATASET,
            Self::Lmcache => a2a_bench_dataset_lmcache::DATASET,
            Self::SweSplice => a2a_bench_dataset_swe_splice::DATASET,
            Self::Cipher => a2a_bench_dataset_cipher::DATASET,
            Self::AiVillage => a2a_bench_dataset_ai_village::DATASET,
            Self::DemoSwarm => a2a_bench_dataset_demo_swarm::DATASET_PREFIX,
        }
    }

    /// The `n` of `<dataset>@<n>` the converter writes.
    pub fn version(self) -> u32 {
        match self {
            Self::Salt => a2a_bench_dataset_salt::VERSION,
            Self::Agentdojo => a2a_bench_dataset_agentdojo::VERSION,
            Self::Tau2 => a2a_bench_dataset_tau2::VERSION,
            Self::CollusionWiki => a2a_bench_dataset_wiki::VERSION,
            Self::SwarmTraces => a2a_bench_dataset_swarm::VERSION,
            Self::OpenSwe => a2a_bench_dataset_open_swe::VERSION,
            Self::Lmcache => a2a_bench_dataset_lmcache::VERSION,
            Self::SweSplice => a2a_bench_dataset_swe_splice::VERSION,
            Self::Cipher => a2a_bench_dataset_cipher::VERSION,
            Self::AiVillage => a2a_bench_dataset_ai_village::VERSION,
            Self::DemoSwarm => a2a_bench_dataset_demo_swarm::VERSION,
        }
    }

    /// Whether the converter composes times on the pace clock (the rest keep
    /// the dataset's own times).
    pub fn paced(self) -> bool {
        matches!(
            self,
            Self::Salt
                | Self::Agentdojo
                | Self::CollusionWiki
                | Self::SwarmTraces
                | Self::OpenSwe
                | Self::SweSplice
                | Self::Cipher
        )
    }
}
