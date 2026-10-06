//! The capture: a swarm run's exchanges and messages in bench format, as
//! crosstalk's adapter (`ct-bench-detect from-export`) writes them from the
//! gateway's own records, carrying the gateway's minted exchange ids and
//! the harness session (and its turn) in each exchange's `client`.
//!
//! A capture is exactly one world in `messages.jsonl` and `exchanges.jsonl`
//! (framed, trailers checked), named by the truth header's `world`, under
//! the dataset of the truth header's scenario. Its declared agents are not
//! read: which agent made which exchange is what the truth says, so the
//! labelled world declares the truth's agents instead.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use a2a_bench_format::check::{InputError, WorldInputs};
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::files::{ExchangeRow, Exchanges, MessageRow, Messages};
use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::jsonl::{FileKind, FileReader, HeaderFields, ReadError};
use a2a_bench_format::message::Message;

/// Why a capture cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("opening {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Read { path: PathBuf, source: ReadError },
    #[error("{path} is dataset {found}, the truth's scenario is {expected}")]
    Dataset {
        path: PathBuf,
        found: DatasetId,
        expected: DatasetId,
    },
    #[error("{path} holds no world")]
    NoWorld { path: PathBuf },
    #[error("{path} holds more than one world (a second: {second})")]
    SecondWorld { path: PathBuf, second: WorldKey },
    #[error("the capture's world is {found}, the truth's is {expected}")]
    World { found: WorldKey, expected: WorldKey },
    #[error("the capture's files disagree: {0}")]
    Inputs(#[from] InputError),
}

/// A checked capture: one world's messages and exchanges.
#[derive(Debug, Clone)]
pub struct Capture {
    pub world: WorldKey,
    pub messages: Vec<Message>,
    /// In file order (time order: the inputs check refuses any other).
    pub exchanges: Vec<Exchange>,
}

fn open<K: FileKind>(path: &Path) -> Result<FileReader<K, BufReader<File>>, CaptureError> {
    let file = File::open(path).map_err(|source| CaptureError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    FileReader::open(BufReader::new(file)).map_err(|source| CaptureError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// The one world section of the file at `path`, checked to be of `dataset`.
fn only_world<K: FileKind>(
    path: &Path,
    dataset: &DatasetId,
) -> Result<(K::World, Vec<K::Row>), CaptureError> {
    let mut reader = open::<K>(path)?;
    let found = reader.header().dataset();
    if found != dataset {
        return Err(CaptureError::Dataset {
            path: path.to_path_buf(),
            found: found.clone(),
            expected: dataset.clone(),
        });
    }
    let read = |source| CaptureError::Read {
        path: path.to_path_buf(),
        source,
    };
    let section = reader
        .next_world()
        .map_err(read)?
        .ok_or_else(|| CaptureError::NoWorld {
            path: path.to_path_buf(),
        })?;
    if let Some(second) = reader.next_world().map_err(read)? {
        use a2a_bench_format::jsonl::Keyed;
        return Err(CaptureError::SecondWorld {
            path: path.to_path_buf(),
            second: second.world.key().clone(),
        });
    }
    Ok((section.world, section.rows))
}

/// Reads and checks the capture at `messages` and `exchanges`: one world,
/// `world`, of `dataset`, every message used and present, every exchange
/// once and in time order (the format's input checks).
pub fn read(
    messages: &Path,
    exchanges: &Path,
    dataset: &DatasetId,
    world: &WorldKey,
) -> Result<Capture, CaptureError> {
    let (messages_world, message_rows) = only_world::<Messages>(messages, dataset)?;
    let (decl, exchange_rows) = only_world::<Exchanges>(exchanges, dataset)?;
    if &decl.key != world {
        return Err(CaptureError::World {
            found: decl.key,
            expected: world.clone(),
        });
    }
    let messages: Vec<Message> = message_rows
        .into_iter()
        .map(|MessageRow::Message(message)| message)
        .collect();
    let exchanges: Vec<Exchange> = exchange_rows
        .into_iter()
        .map(|ExchangeRow::Exchange(exchange)| exchange)
        .collect();
    // The format's checks over the capture as the adapter wrote it.
    WorldInputs::new(
        &messages_world.key,
        messages.clone(),
        decl,
        exchanges.clone(),
    )?;
    Ok(Capture {
        world: world.clone(),
        messages,
        exchanges,
    })
}
