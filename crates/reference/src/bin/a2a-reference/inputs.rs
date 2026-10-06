//! Reading the input view: the manifest, and the two framed files world by
//! world in lockstep, each world checked against the manifest's order.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::slice;

use a2a_bench_format::check::{InputError, WorldInputs};
use a2a_bench_format::files::{ExchangeRow, Exchanges, MessageRow, Messages};
use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::jsonl::{FileKind, FileReader, HeaderFields, WorldSection};
use a2a_bench_format::manifest::{Manifest, WorldEntry};
use anyhow::{Context, anyhow, bail, ensure};

pub const MANIFEST: &str = "manifest.json";
pub const MESSAGES: &str = "messages.jsonl";
pub const EXCHANGES: &str = "exchanges.jsonl";

pub fn manifest(dir: &Path) -> anyhow::Result<Manifest> {
    let path = dir.join(MANIFEST);
    let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("reading {}", path.display()))
}

fn open<K: FileKind>(
    dir: &Path,
    name: &str,
    manifest: &Manifest,
) -> anyhow::Result<FileReader<K, BufReader<File>>> {
    let path = dir.join(name);
    let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
    let reader = FileReader::open(BufReader::new(file))
        .with_context(|| format!("reading {}", path.display()))?;
    let dataset: &DatasetId = HeaderFields::dataset(reader.header());
    ensure!(
        dataset == &manifest.dataset,
        "{name} is dataset {dataset}, the manifest says {}",
        manifest.dataset
    );
    Ok(reader)
}

/// One world's rows from both files.
pub struct WorldFiles {
    pub key: WorldKey,
    pub messages: WorldSection<Messages>,
    pub exchanges: WorldSection<Exchanges>,
}

impl WorldFiles {
    /// The world's inputs, checked together.
    pub fn into_inputs(self) -> Result<WorldInputs, InputError> {
        let messages = self
            .messages
            .rows
            .into_iter()
            .map(|row| match row {
                MessageRow::Message(message) => message,
            })
            .collect();
        let exchanges = self
            .exchanges
            .rows
            .into_iter()
            .map(|row| match row {
                ExchangeRow::Exchange(exchange) => exchange,
            })
            .collect();
        WorldInputs::new(
            &self.messages.world.key,
            messages,
            self.exchanges.world,
            exchanges,
        )
    }
}

/// The two files, read in lockstep, in the manifest's world order.
pub struct Worlds<'m> {
    messages: FileReader<Messages, BufReader<File>>,
    exchanges: FileReader<Exchanges, BufReader<File>>,
    expected: slice::Iter<'m, WorldEntry>,
}

impl<'m> Worlds<'m> {
    pub fn open(dir: &Path, manifest: &'m Manifest) -> anyhow::Result<Self> {
        Ok(Self {
            messages: open(dir, MESSAGES, manifest)?,
            exchanges: open(dir, EXCHANGES, manifest)?,
            expected: manifest.worlds.iter(),
        })
    }

    /// The next world, `None` after the last; an error when the files or
    /// the manifest disagree on which world comes next.
    pub fn next_world(&mut self) -> anyhow::Result<Option<WorldFiles>> {
        let messages = self
            .messages
            .next_world()
            .with_context(|| format!("reading {MESSAGES}"))?;
        let exchanges = self
            .exchanges
            .next_world()
            .with_context(|| format!("reading {EXCHANGES}"))?;
        let (messages, exchanges) = match (messages, exchanges) {
            (None, None) => {
                if let Some(entry) = self.expected.next() {
                    bail!(
                        "the manifest lists world {}, the files end before it",
                        entry.key
                    );
                }
                return Ok(None);
            }
            (Some(messages), Some(exchanges)) => (messages, exchanges),
            (Some(messages), None) => bail!(
                "{MESSAGES} holds world {} past the end of {EXCHANGES}",
                messages.world.key
            ),
            (None, Some(exchanges)) => bail!(
                "{EXCHANGES} holds world {} past the end of {MESSAGES}",
                exchanges.world.key
            ),
        };
        let key = exchanges.world.key.clone();
        ensure!(
            messages.world.key == key,
            "world order differs: {MESSAGES} holds {} where {EXCHANGES} holds {key}",
            messages.world.key
        );
        let entry = self
            .expected
            .next()
            .ok_or_else(|| anyhow!("world {key} is not in the manifest"))?;
        ensure!(
            entry.key == key,
            "world order differs: the manifest lists {} where the files hold {key}",
            entry.key
        );
        Ok(Some(WorldFiles {
            key,
            messages,
            exchanges,
        }))
    }
}
