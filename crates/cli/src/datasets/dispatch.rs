//! One export per dataset: build the converter's `Options` from the flags,
//! open its source, write the export through `corpus::export`, then record
//! the source digest of the files the converter read.
//!
//! The files read are known only once the worlds have been produced, so the
//! manifest is written by the export with a zero digest and rewritten here
//! with the real one before anything reads it.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use a2a_bench_corpus::export::{
    DigestError, ExportError, FilesRead, ManifestInfo, export as write_export, write_manifest,
};
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::split::Selection;
use a2a_bench_dataset_agentdojo as agentdojo;
use a2a_bench_dataset_ai_village as ai_village;
use a2a_bench_dataset_cipher as cipher;
use a2a_bench_dataset_demo_swarm as demo_swarm;
use a2a_bench_dataset_lmcache as lmcache;
use a2a_bench_dataset_open_swe as open_swe;
use a2a_bench_dataset_salt as salt;
use a2a_bench_dataset_swarm as swarm;
use a2a_bench_dataset_swe_splice as swe_splice;
use a2a_bench_dataset_tau2 as tau2;
use a2a_bench_dataset_wiki as wiki;
use a2a_bench_format::ids::Digest;
use a2a_bench_format::manifest::{Manifest, Setting, Source};

use super::{DatasetFlags, DatasetName, FlagError};
use crate::repo;

#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error(transparent)]
    Flags(#[from] FlagError),
    #[error("opening salt: {0}")]
    Salt(#[from] salt::SaltError),
    #[error("opening agentdojo: {0}")]
    AgentDojo(#[from] agentdojo::AgentDojoError),
    #[error("opening tau2: {0}")]
    Tau2(#[from] tau2::Tau2Error),
    #[error("opening collusion-wiki: {0}")]
    Wiki(#[from] wiki::WikiError),
    #[error("opening swarm-traces: {0}")]
    Swarm(#[from] swarm::SwarmError),
    #[error("opening open_swe: {0}")]
    OpenSwe(#[from] open_swe::OpenSweError),
    #[error("opening lmcache: {0}")]
    Lmcache(#[from] lmcache::LmcacheError),
    #[error("opening swe_splice: {0}")]
    Splice(#[from] swe_splice::SpliceError),
    #[error("opening cipher: {0}")]
    Cipher(#[from] cipher::CipherError),
    #[error("opening ai-village: {0}")]
    AiVillage(#[from] ai_village::Error),
    #[error("demo-swarm: {0}")]
    DemoSwarm(#[from] demo_swarm::Error),
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error("the source digest: {0}")]
    Digest(#[from] DigestError),
    #[error("resolving {path}: {source}")]
    Resolve {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// What to export and where.
#[derive(Debug, Clone, Copy)]
pub struct ExportRequest<'a> {
    pub dataset: DatasetName,
    pub flags: &'a DatasetFlags,
    /// The dataset's directory.
    pub dataset_dir: &'a Path,
    /// The manifest's `source.path`.
    pub source_path: &'a str,
    /// The dataset's actual revision.
    pub revision: &'a str,
    pub out: &'a Path,
    pub selection: &'a Selection,
}

/// What an export wrote.
#[derive(Debug, Clone)]
pub struct ExportSummary {
    pub manifest: Manifest,
    /// Positions (in the source's stream) of worlds that failed to convert;
    /// the errors are in the log.
    pub failed: Vec<usize>,
    /// Worlds the split left out.
    pub left_out: usize,
    /// Dev-listed worlds the source never produced.
    pub missing: usize,
    /// Files the converter read.
    pub files_read: usize,
}

/// Exports `request.dataset` (any but demo-swarm, see [`export_demo_swarm`]).
pub fn export(request: &ExportRequest<'_>) -> Result<ExportSummary, DatasetError> {
    let flags = request.flags;
    flags.check(request.dataset)?;
    let root = request.dataset_dir;
    let paced = || -> Result<_, DatasetError> {
        let pace = flags.pace()?;
        Ok((pace, pace.settings()))
    };
    match request.dataset {
        DatasetName::Salt => {
            let options = salt::Options {
                limit: flags.limit,
                include: flags.include.clone(),
            };
            let (pace, settings) = paced()?;
            let source = salt::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::Agentdojo => {
            let options = agentdojo::Options {
                limit: flags.limit,
                include: flags.include.clone(),
            };
            let (pace, settings) = paced()?;
            let source = agentdojo::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::Tau2 => {
            let options = tau2::Options {
                limit: flags.limit,
                include: flags.include.clone(),
            };
            let source = tau2::source(root, &options)?;
            write(request, source, options.settings(), BTreeMap::new(), |s| {
                s.files_read()
            })
        }
        DatasetName::CollusionWiki => {
            let options = wiki::Options {
                families: flags.family.clone(),
                wikis: flags.wiki.clone(),
                min_agents: flags.min_agents,
                max_agents: flags.max_agents,
                limit: flags.limit,
                demo: flags.demo,
            };
            let (pace, settings) = paced()?;
            let source = wiki::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::SwarmTraces => {
            let options = swarm::Options { limit: flags.limit };
            let (pace, settings) = paced()?;
            let source = swarm::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::OpenSwe => {
            let options = open_swe::Options {
                limit: flags.limit,
                include: flags.include.clone(),
                count: flags.count,
                agents_per_world: flags.agents_per_world.unwrap_or(open_swe::AGENTS_PER_WORLD),
            };
            let (pace, settings) = paced()?;
            let source = open_swe::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::Lmcache => {
            let options = lmcache::Options {
                limit: flags.limit,
                include: flags.include.clone(),
                count: flags.count,
                agents_per_world: flags.agents_per_world.unwrap_or(lmcache::AGENTS_PER_WORLD),
            };
            let source = lmcache::source(root, &options)?;
            write(request, source, options.settings(), BTreeMap::new(), |s| {
                s.files_read()
            })
        }
        DatasetName::SweSplice => {
            let options = swe_splice::Options {
                limit: flags.limit,
                include: flags.include.clone(),
                count: flags.count.unwrap_or(swe_splice::SPLICES),
                seed: flags.seed(),
            };
            let (pace, settings) = paced()?;
            let source = swe_splice::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::Cipher => {
            let options = cipher::Options {
                limit: flags.limit,
                include: flags.include.clone(),
                count: flags.count.unwrap_or(cipher::PAIRS_PER_CIPHER),
                seed: flags.seed(),
            };
            let (pace, settings) = paced()?;
            let source = cipher::source(root, &options, pace)?;
            write(request, source, options.settings(), settings, |s| {
                s.files_read()
            })
        }
        DatasetName::AiVillage => {
            let options = flags.ai_village()?;
            let source = ai_village::source(root, &options)?;
            write(request, source, options.settings(), BTreeMap::new(), |s| {
                s.files_read()
            })
        }
        DatasetName::DemoSwarm => Err(DatasetError::Flags(FlagError::Missing {
            dataset: DatasetName::DemoSwarm.id(),
            flag: "inputs",
        })),
    }
}

/// Exports `source` through the corpus writer, then records the digest of
/// the files it read.
fn write<S: TraceSource>(
    request: &ExportRequest<'_>,
    mut source: S,
    selection: BTreeMap<String, Setting>,
    pace: BTreeMap<String, Setting>,
    files_read: impl Fn(&S) -> &FilesRead,
) -> Result<ExportSummary, DatasetError> {
    let info = ManifestInfo {
        dataset: source.dataset().clone(),
        dataset_version: request.dataset.version(),
        source: Source {
            path: request.source_path.to_owned(),
            revision: request.revision.to_owned(),
            digest: Digest::from_bytes([0; 32]),
        },
        converter: repo::converter(),
        selection,
        pace,
    };
    let exported = write_export(&mut source, request.out, info, request.selection)?;
    let read = files_read(&source);
    let mut manifest = exported.manifest;
    manifest.source.digest = read.digest(request.dataset_dir)?;
    write_manifest(request.out, &manifest)?;
    Ok(ExportSummary {
        manifest,
        failed: exported
            .failures
            .iter()
            .map(|failure| failure.position)
            .collect(),
        left_out: exported.left_out,
        missing: exported.missing.len(),
        files_read: read.len(),
    })
}

/// The deepest directory holding both `a` and `b` (both absolute).
fn common_ancestor(a: &Path, b: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for (x, y) in a.components().zip(b.components()) {
        if x != y {
            break;
        }
        out.push(x.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(Component::RootDir.as_os_str());
    }
    out
}

fn resolve(path: &Path) -> Result<PathBuf, DatasetError> {
    path.canonicalize().map_err(|source| DatasetError::Resolve {
        path: path.to_path_buf(),
        source,
    })
}

/// Exports a labelled demo-swarm run: the capture in `flags.inputs`
/// (`messages.jsonl`, `exchanges.jsonl`, `manifest.json`) and
/// `flags.truth`. The manifest is the capture's plus truth (source,
/// converter, selection are the capture's); the bench's version and the
/// truth's path and BLAKE3 go to `diagnostics.json`.
pub fn export_demo_swarm(flags: &DatasetFlags, out: &Path) -> Result<ExportSummary, DatasetError> {
    flags.check(DatasetName::DemoSwarm)?;
    let missing = |flag| {
        DatasetError::Flags(FlagError::Missing {
            dataset: DatasetName::DemoSwarm.id(),
            flag,
        })
    };
    let inputs_dir = flags.inputs.as_deref().ok_or_else(|| missing("inputs"))?;
    let truth = flags.truth.as_deref().ok_or_else(|| missing("truth"))?;
    let inputs_abs = resolve(inputs_dir)?;
    let truth_abs = resolve(truth)?;
    let root = common_ancestor(&inputs_abs, truth_abs.parent().unwrap_or(&truth_abs));
    let inputs = demo_swarm::Inputs {
        root,
        truth: truth_abs,
        messages: inputs_abs.join("messages.jsonl"),
        exchanges: inputs_abs.join("exchanges.jsonl"),
        manifest: inputs_abs.join(demo_swarm::CAPTURE_MANIFEST_FILE),
    };
    let defaults = demo_swarm::Options::default();
    let options = demo_swarm::Options {
        run_lead_ms: flags.run_lead_ms.unwrap_or(defaults.run_lead_ms),
        run_slack_ms: flags.run_slack_ms.unwrap_or(defaults.run_slack_ms),
    };
    let exported = demo_swarm::write_export(&inputs, out, &options, repo::converter())?;
    Ok(ExportSummary {
        manifest: exported.manifest,
        failed: Vec::new(),
        left_out: exported.left_out,
        missing: exported.missing.len(),
        files_read: 3,
    })
}
