//! swarm-traces: a decoder test corpus.
//!
//! The export (`redacted.jsonl[.gz]`) records encoded payloads agents passed
//! each other and, for many, a recovered-text or response child. **The
//! payloads are real attack content and are treated purely as text: never
//! executed, never fetched, never printed or logged.** No dataset bytes
//! live in the repository; the fixtures are benign synthetic strings that
//! mimic the encoding structure only.
//!
//! The converter extracts candidate tokens from each payload ([`tokens`]),
//! decodes each through a chain of codecs ([`codec`]) and, for every token
//! that decodes to printable text of at least 24 bytes and 20 letters or
//! digits through a chain every layer of which is a format codec, builds a
//! two-agent world ([`build`]). Decode chains are counted per run
//! ([`tally`]): chains, counts and lengths only. This is crosstalk-eval's
//! converter at 7f8a2fb, ported for parity (version 1).

pub mod build;
pub mod codec;
pub mod error;
mod messages;
pub mod read;
pub mod tally;
pub mod tokens;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::manifest::Setting;

use build::TokenWorld;
use codec::decode;
pub use error::SwarmError;
pub use tally::{ChainStats, ChainTally};

/// The dataset's id (crosstalk-eval's).
pub const DATASET: &str = "swarm-traces";
/// The converter's dataset version.
pub const VERSION: u32 = 1;
/// The logical file rows are cited from, gzipped or not.
pub const PAYLOADS_FILE: &str = "redacted.jsonl";
/// The shortest decoded plaintext worth a label (the reference matcher's
/// span floor).
const MIN_PLAINTEXT: usize = 24;
const MIN_WORD_CHARS: usize = 20;

/// ct-eval's swarm flag (`--limit`), with its default (none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Emit at most this many worlds.
    pub limit: Option<usize>,
}

impl Options {
    /// The manifest's `selection`: `limit` when set.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        self.limit
            .map(|limit| {
                (
                    "limit".to_owned(),
                    Setting::Int(i64::try_from(limit).unwrap_or(i64::MAX)),
                )
            })
            .into_iter()
            .collect()
    }
}

/// swarm-traces as a stream of worlds, one per decodable token.
pub struct SwarmSource {
    dataset: DatasetId,
    worlds: Vec<TokenWorld>,
    tally: ChainTally,
    files: FilesRead,
    filter: WorldFilter,
    pace: Pace,
}

/// Reads the export under `root` and plans its worlds under `options`, with
/// calls `pace` apart.
pub fn source(root: &Path, options: &Options, pace: Pace) -> Result<SwarmSource, SwarmError> {
    SwarmSource::open(root, options, pace)
}

impl SwarmSource {
    /// Reads the export under `root` and plans one world per decodable
    /// token, stopping once `options.limit` worlds are planned.
    pub fn open(root: &Path, options: &Options, pace: Pace) -> Result<Self, SwarmError> {
        let mut files = FilesRead::new();
        let rows = read::rows(root, &mut files)?;
        // Which payloads have a recovered-text or response child.
        let mut corroborated: BTreeSet<&str> = BTreeSet::new();
        for row in &rows {
            if matches!(row.kind.as_str(), "recovered_text" | "response")
                && let Some(parent) = &row.parent_id
            {
                corroborated.insert(parent.as_str());
            }
        }
        let mut worlds = Vec::new();
        let mut tally = ChainTally::default();
        'rows: for row in &rows {
            if row.kind != "payload" {
                continue;
            }
            tally.payloads += 1;
            let is_corroborated = corroborated.contains(row.id.as_str());
            let mut seen = BTreeSet::new();
            for (index, token) in tokens::tokens(&row.text).into_iter().enumerate() {
                if !seen.insert(token.clone()) {
                    continue;
                }
                tally.candidates += 1;
                let Some(decoded) = decode(&token) else {
                    continue;
                };
                if decoded.text.len() < MIN_PLAINTEXT || word_chars(&decoded.text) < MIN_WORD_CHARS
                {
                    continue;
                }
                tally.add(&decoded, token.len(), is_corroborated);
                if decoded.codecs().is_none() {
                    // A layer with no format codec: a reported gap, not a world.
                    continue;
                }
                worlds.push(TokenWorld {
                    payload_id: row.id.clone(),
                    token_index: index,
                    token,
                    corroborated: is_corroborated,
                });
                if options.limit.is_some_and(|limit| worlds.len() >= limit) {
                    break 'rows;
                }
            }
        }
        tracing::debug!(
            dataset = DATASET,
            payloads = tally.payloads,
            candidates = tally.candidates,
            worlds = worlds.len(),
            "planned swarm-traces worlds"
        );
        Ok(Self {
            dataset: DatasetId::new(DATASET)?,
            worlds,
            tally,
            files,
            filter: WorldFilter::All,
            pace,
        })
    }

    /// Decode-chain counts over the payloads read: chains and lengths only,
    /// never token or plaintext bytes.
    pub fn tally(&self) -> &ChainTally {
        &self.tally
    }

    pub fn world_count(&self) -> usize {
        self.worlds.len()
    }

    /// The files read, relative to the root, for the source digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }
}

impl TraceSource for SwarmSource {
    type Error = SwarmError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn select(&mut self, filter: &WorldFilter) {
        self.filter = filter.clone();
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, SwarmError>> + '_ {
        self.worlds.iter().filter_map(|plan| {
            let key = match WorldKey::new(plan.key()) {
                Ok(key) => key,
                Err(error) => return Some(Err(error.into())),
            };
            self.filter
                .keeps(&key)
                .then(|| build::world(&self.dataset, plan, self.pace))
        })
    }
}

/// Letters and digits in `text` (ASCII alphanumerics and any non-ASCII
/// byte), as the reference matcher counts them.
fn word_chars(text: &str) -> usize {
    text.bytes()
        .filter(|b| b.is_ascii_alphanumeric() || *b >= 0x80)
        .count()
}
