//! `a2a-bench export`: a dataset to an export directory.
//!
//! ```text
//! export --dataset <id> --out <dir> [--root <data root>] [--dataset-dir <dir>]
//!        [--config datasets.toml] [--splits <dir>] [--version N]
//!        [--split dev|holdout --release <detector>@<tag>] [--allow-revision]
//!        [dataset flags…]
//! export --dataset demo-swarm --inputs <dir> --truth <truth.jsonl> --out <dir>
//!        [--split holdout --release <detector>@<version> [--seed N] [--splits <dir>]]
//! ```
//!
//! The dataset's directory comes from `--dataset-dir`, else `datasets.toml`
//! (its `root`, or `--root`, joined with the dataset's `path`). Its actual
//! revision is checked against the pin (design §8). The split comes from
//! `splits/<dataset>@<n>.dev.toml` (dev, or unsplit when there is none); a
//! holdout export also writes or checks `splits/<dataset>@<n>.holdout.commit`.
//!
//! demo-swarm's holdout unit is a whole run ([`holdout::demo_swarm`]): the
//! capture's run, made with a holdout seed, exported for the release, its
//! entry added to or checked against `splits/demo-swarm@1.holdout.commit`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::config::{ConfigError, DatasetsConfig, expand, home};
use a2a_bench_corpus::split::{Release, Selection, SplitError};
use a2a_bench_format::ids::{DatasetId, InvalidKey};
use clap::{Args, ValueEnum};

use a2a_bench_dataset_demo_swarm as demo_swarm;

use crate::datasets::{
    DatasetError, DatasetFlags, DatasetName, ExportRequest, ExportSummary, demo_swarm_inputs,
    export as export_dataset, export_demo_swarm,
};
use crate::holdout::demo_swarm::{self as swarm_holdout, Entry, RunSeed, SeedError};
use crate::holdout::{self, Committed, HoldoutError};
use crate::repo;
use crate::revision::{Pin, Revision, RevisionError, check_pin, source_revision};

/// Which split to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum SplitArg {
    #[default]
    Dev,
    Holdout,
}

#[derive(Debug, Clone, Args)]
pub struct ExportArgs {
    /// The dataset (its bench id; ct-eval's names are accepted too).
    #[arg(long, value_enum)]
    pub dataset: DatasetName,
    /// The export directory (empty or new).
    #[arg(long)]
    pub out: PathBuf,
    /// The data root (default: datasets.toml's `root`).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// The dataset's own directory (overrides --root and datasets.toml).
    #[arg(long)]
    pub dataset_dir: Option<PathBuf>,
    /// The datasets config (default: the repository's datasets.toml).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// The splits directory (default: the repository's splits/).
    #[arg(long)]
    pub splits: Option<PathBuf>,
    /// The dataset version to write; only the converter's own is possible.
    #[arg(long = "version")]
    pub dataset_version: Option<u32>,
    #[arg(long, value_enum, default_value_t = SplitArg::Dev)]
    pub split: SplitArg,
    /// The release a holdout export is for: <detector>@<tag>.
    #[arg(long)]
    pub release: Option<String>,
    /// Export even when the dataset's revision is not the pinned one.
    #[arg(long)]
    pub allow_revision: bool,
    #[command(flatten)]
    pub flags: DatasetFlags,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportCommandError {
    #[error("{dataset} is written at version {writes}, not {asked}")]
    Version {
        dataset: &'static str,
        asked: u32,
        writes: u32,
    },
    #[error("--split holdout needs --release <detector>@<tag>")]
    ReleaseRequired,
    #[error("--release applies only to --split holdout")]
    ReleaseWithoutHoldout,
    #[error("--seed applies only to a demo-swarm holdout export")]
    SeedWithoutHoldout,
    #[error(transparent)]
    Seed(#[from] SeedError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Split(#[from] SplitError),
    #[error(transparent)]
    Revision(#[from] RevisionError),
    #[error(transparent)]
    Holdout(#[from] HoldoutError),
    #[error(transparent)]
    Dataset(#[from] Box<DatasetError>),
    #[error(transparent)]
    Key(#[from] InvalidKey),
}

/// What an export did.
#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub out: PathBuf,
    pub summary: ExportSummary,
    pub revision: String,
    pub pin: Pin,
    /// The holdout commitment, for a holdout export.
    pub committed: Option<Committed>,
    /// The run's seed, for a demo-swarm holdout export.
    pub seed: Option<RunSeed>,
}

impl ExportOutcome {
    /// Counts only.
    pub fn render(&self) -> String {
        let m = &self.summary.manifest;
        let exchanges: u64 = m.worlds.iter().map(|w| w.exchanges).sum();
        let labels: u64 = m.worlds.iter().filter_map(|w| w.labels).sum();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "exported {}@{} ({}) to {}: {} worlds, {} exchanges, {} label rows; {} failed, {} left out by the split, {} dev-listed missing; {} files read",
            m.dataset,
            m.dataset_version,
            match m.split {
                a2a_bench_format::manifest::Split::Dev => "dev",
                a2a_bench_format::manifest::Split::Holdout => "holdout",
            },
            self.out.display(),
            m.worlds.len(),
            exchanges,
            labels,
            self.summary.failed.len(),
            self.summary.left_out,
            self.summary.missing,
            self.summary.files_read,
        );
        let pin = match &self.pin {
            Pin::Unpinned => "not pinned".to_owned(),
            Pin::Matches => "pinned".to_owned(),
            Pin::Overridden { pinned } => format!("pinned {pinned}, overridden"),
        };
        let _ = writeln!(out, "source revision {} ({pin})", self.revision);
        match &self.committed {
            Some(Committed::Recorded { path, commitment }) => {
                let _ = writeln!(
                    out,
                    "holdout commitment {commitment} recorded in {}",
                    path.display()
                );
            }
            Some(Committed::Matched { path, commitment }) => {
                let _ = writeln!(
                    out,
                    "holdout commitment {commitment} matches {}",
                    path.display()
                );
            }
            None => {}
        }
        if let Some(seed) = &self.seed {
            let _ = writeln!(
                out,
                "holdout run seed {} (from {})",
                seed.seed,
                match &seed.from {
                    swarm_holdout::SeedFrom::BenchEnv(path) => path.display().to_string(),
                    swarm_holdout::SeedFrom::Flag => "--seed".to_owned(),
                }
            );
        }
        out
    }
}

fn load_config(path: Option<&Path>) -> Result<DatasetsConfig, ConfigError> {
    match path {
        Some(path) => DatasetsConfig::load(path),
        None => {
            let path = repo::datasets_config();
            if path.is_file() {
                DatasetsConfig::load(&path)
            } else {
                Ok(DatasetsConfig::default())
            }
        }
    }
}

/// Runs `export` (module docs).
pub fn export(args: &ExportArgs) -> Result<ExportOutcome, ExportCommandError> {
    let dataset = args.dataset;
    let version = dataset.version();
    if let Some(asked) = args.dataset_version
        && asked != version
    {
        return Err(ExportCommandError::Version {
            dataset: dataset.id(),
            asked,
            writes: version,
        });
    }
    let release: Option<Release> = match (args.split, &args.release) {
        (SplitArg::Dev, None) => None,
        (SplitArg::Dev, Some(_)) => return Err(ExportCommandError::ReleaseWithoutHoldout),
        (SplitArg::Holdout, None) => return Err(ExportCommandError::ReleaseRequired),
        (SplitArg::Holdout, Some(text)) => Some(text.parse()?),
    };
    if dataset == DatasetName::DemoSwarm {
        return match release {
            None => export_swarm_dev(args),
            Some(release) => export_swarm_holdout(args, &release),
        };
    }
    if release.is_some() {
        holdout::outside_repository(&args.out)?;
    }

    let config = load_config(args.config.as_deref())?;
    let home = home();
    let data_root = args
        .root
        .clone()
        .unwrap_or_else(|| config.root(home.as_deref()));
    let entry = config.datasets.get(dataset.id());
    let (dataset_dir, source_path) = match (&args.dataset_dir, entry) {
        (Some(dir), _) => (dir.clone(), dir.display().to_string()),
        (None, Some(entry)) => {
            let path = expand(&entry.path, home.as_deref());
            let dir = if path.is_absolute() {
                path
            } else {
                data_root.join(path)
            };
            (dir, entry.path.clone())
        }
        (None, None) => {
            return Err(ConfigError::UnknownDataset(dataset.id().to_owned()).into());
        }
    };
    let revision: Revision = source_revision(&dataset_dir, Some(&data_root))?;
    let pinned = entry.and_then(|entry| entry.pinned_revision());
    let pin = check_pin(pinned, revision.as_str(), args.allow_revision)?;
    if let Pin::Overridden { pinned } = &pin {
        tracing::warn!(dataset = dataset.id(), pinned = %pinned, actual = %revision, "source revision differs from the pin; exporting the actual one (--allow-revision)");
    }

    let splits = args.splits.clone().unwrap_or_else(repo::splits_dir);
    let id = DatasetId::new(dataset.id())?;
    let selection = match &release {
        None => Selection::dev(&splits, &id, version)?,
        Some(release) => Selection::holdout(&splits, &id, version, release.clone())?,
    };
    tracing::info!(
        dataset = dataset.id(),
        version,
        dir = %dataset_dir.display(),
        revision = %revision,
        split = ?args.split,
        "exporting"
    );
    let summary = export_dataset(&ExportRequest {
        dataset,
        flags: &args.flags,
        dataset_dir: &dataset_dir,
        source_path: &source_path,
        revision: revision.as_str(),
        out: &args.out,
        selection: &selection,
    })
    .map_err(Box::new)?;
    let committed = match release {
        None => None,
        Some(_) => {
            let path = holdout::commit_path(&splits, &id, version);
            let commitment = holdout::commitment(&summary.manifest)?;
            Some(holdout::record_or_check(&path, commitment)?)
        }
    };
    Ok(ExportOutcome {
        out: args.out.clone(),
        summary,
        revision: revision.as_str().to_owned(),
        pin,
        committed,
        seed: None,
    })
}

/// A demo-swarm dev export: the capture plus truth, as before holdouts.
fn export_swarm_dev(args: &ExportArgs) -> Result<ExportOutcome, ExportCommandError> {
    if args.flags.swarm_seed.is_some() {
        return Err(ExportCommandError::SeedWithoutHoldout);
    }
    let summary = export_demo_swarm(&args.flags, &args.out, None).map_err(Box::new)?;
    Ok(ExportOutcome {
        out: args.out.clone(),
        revision: summary.manifest.source.revision.clone(),
        summary,
        pin: Pin::Unpinned,
        committed: None,
        seed: None,
    })
}

/// A demo-swarm holdout export (module docs): `--out` outside every
/// repository, a holdout seed that is the truth's, the export marked for
/// `release`, then the run's commitment entry.
fn export_swarm_holdout(
    args: &ExportArgs,
    release: &Release,
) -> Result<ExportOutcome, ExportCommandError> {
    holdout::outside_repository(&args.out)?;
    let (inputs_dir, inputs) = demo_swarm_inputs(&args.flags).map_err(Box::new)?;
    let truth = demo_swarm::source::read_truth(&inputs.truth)
        .map_err(|error| Box::new(DatasetError::DemoSwarm(error)))?;
    let seed = swarm_holdout::run_seed(&inputs_dir, args.flags.swarm_seed, truth.header.seed)?;
    tracing::info!(
        run = %truth.header.run,
        seed = seed.seed,
        release = %release,
        "exporting a demo-swarm holdout run"
    );
    let summary = export_demo_swarm(&args.flags, &args.out, Some(release)).map_err(Box::new)?;
    let splits = args.splits.clone().unwrap_or_else(repo::splits_dir);
    let id = DatasetId::new(DatasetName::DemoSwarm.id())?;
    let path = holdout::commit_path(&splits, &id, DatasetName::DemoSwarm.version());
    let entry = Entry::new(
        &truth.header.run,
        seed.seed,
        holdout::commitment(&summary.manifest)?,
    )?;
    let committed = swarm_holdout::record_or_check(&path, &entry)?;
    Ok(ExportOutcome {
        out: args.out.clone(),
        revision: summary.manifest.source.revision.clone(),
        summary,
        pin: Pin::Unpinned,
        committed: Some(committed),
        seed: Some(seed),
    })
}
