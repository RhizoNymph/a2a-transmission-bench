//! JSONL framing shared by every file: a header, then one section per world
//! (a `world` row and its rows), then a trailer whose digest covers every
//! line before it. Worlds come in the same order in every file of an
//! export, so a reader holds one world at a time.

mod read;
mod write;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::ids::{DatasetId, Digest, WorldKey};
use crate::version::Format;

pub use read::{FileReader, ReadError, WorldSection};
pub use write::{FileWriter, WriteError};

/// The BLAKE3 derive-key context of a file's digest.
pub const FILE_DIGEST_CONTEXT: &str = "a2a-bench/1 file";

/// One kind of file: its name in headers, its header, its world row and
/// its rows.
pub trait FileKind {
    /// `messages`, `exchanges`, `labels` or `predictions`.
    const NAME: &'static str;
    type Header: Serialize + DeserializeOwned + Clone + HeaderFields;
    /// The `world` row; it names the world's key.
    type World: Serialize + DeserializeOwned + Clone + Keyed;
    /// A row inside a world section, tagged by `kind`.
    type Row: Serialize + DeserializeOwned;
}

/// The fields every header has.
pub trait HeaderFields {
    fn format(&self) -> Format;
    fn file(&self) -> &str;
    fn dataset(&self) -> &DatasetId;
}

/// A world row's key.
pub trait Keyed {
    fn key(&self) -> &WorldKey;
}

/// The header of a file that carries nothing beyond the common fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicHeader {
    pub format: Format,
    pub file: String,
    pub dataset: DatasetId,
}

impl BasicHeader {
    /// The current format's header for file kind `K`.
    pub fn new<K: FileKind>(dataset: DatasetId) -> Self {
        Self {
            format: crate::version::FORMAT,
            file: K::NAME.to_owned(),
            dataset,
        }
    }
}

impl HeaderFields for BasicHeader {
    fn format(&self) -> Format {
        self.format
    }

    fn file(&self) -> &str {
        &self.file
    }

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }
}

/// The trailer line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trailer {
    pub worlds: u64,
    pub rows: u64,
    pub digest: Digest,
}

/// A framing line: header, world opener or trailer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Frame<H, W> {
    Header(H),
    World(W),
    Trailer(Trailer),
}
