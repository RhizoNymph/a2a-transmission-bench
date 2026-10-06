//! The three JSONL files of an export, written in lockstep, and the
//! manifest file.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use a2a_bench_format::files::{
    ExchangeRow, Exchanges, Labels, LabelsWorld, MessageRow, Messages, WorldOnly,
};
use a2a_bench_format::ids::{DatasetId, Digest};
use a2a_bench_format::jsonl::{BasicHeader, FileKind, FileWriter, WriteError};
use a2a_bench_format::manifest::Manifest;

use super::ExportError;
use crate::world::World;

/// The file names of an export.
pub const MANIFEST_FILE: &str = "manifest.json";
pub const MESSAGES_FILE: &str = "messages.jsonl";
pub const EXCHANGES_FILE: &str = "exchanges.jsonl";
pub const LABELS_FILE: &str = "labels.jsonl";

type Out = BufWriter<File>;

fn create(path: &Path) -> Result<Out, ExportError> {
    File::create(path)
        .map(BufWriter::new)
        .map_err(|source| ExportError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn open<K: FileKind<Header = BasicHeader>>(
    dir: &Path,
    name: &'static str,
    dataset: &DatasetId,
) -> Result<FileWriter<K, Out>, ExportError> {
    let out = create(&dir.join(name))?;
    FileWriter::new(out, &BasicHeader::new::<K>(dataset.clone()))
        .map_err(|source| ExportError::Write { file: name, source })
}

/// The digests of the three files, from their trailers.
pub(super) struct Trailers {
    pub messages: Digest,
    pub exchanges: Digest,
    pub labels: Digest,
}

pub(super) struct Writers {
    messages: FileWriter<Messages, Out>,
    exchanges: FileWriter<Exchanges, Out>,
    labels: FileWriter<Labels, Out>,
}

fn in_file(file: &'static str) -> impl Fn(WriteError) -> ExportError {
    move |source| ExportError::Write { file, source }
}

impl Writers {
    pub fn create(dir: &Path, dataset: &DatasetId) -> Result<Self, ExportError> {
        Ok(Self {
            messages: open(dir, MESSAGES_FILE, dataset)?,
            exchanges: open(dir, EXCHANGES_FILE, dataset)?,
            labels: open(dir, LABELS_FILE, dataset)?,
        })
    }

    /// One world's section in each file: messages once each in first-use
    /// order, the declaration then exchanges in time order, the coverage
    /// then labels.
    pub fn world(&mut self, world: &World) -> Result<(), ExportError> {
        let key = world.key().clone();
        self.messages
            .world(&WorldOnly { key: key.clone() })
            .map_err(in_file(MESSAGES_FILE))?;
        for message in world.messages_in_order() {
            self.messages
                .row(&MessageRow::Message(message.clone()))
                .map_err(in_file(MESSAGES_FILE))?;
        }
        self.exchanges
            .world(world.decl())
            .map_err(in_file(EXCHANGES_FILE))?;
        for exchange in world.exchanges() {
            self.exchanges
                .row(&ExchangeRow::Exchange(exchange.clone()))
                .map_err(in_file(EXCHANGES_FILE))?;
        }
        self.labels
            .world(&LabelsWorld {
                key,
                coverage: world.coverage(),
            })
            .map_err(in_file(LABELS_FILE))?;
        for label in world.labels() {
            self.labels.row(label).map_err(in_file(LABELS_FILE))?;
        }
        Ok(())
    }

    /// Writes the trailers and flushes.
    pub fn finish(self) -> Result<Trailers, ExportError> {
        let (_, messages) = self.messages.finish().map_err(in_file(MESSAGES_FILE))?;
        let (_, exchanges) = self.exchanges.finish().map_err(in_file(EXCHANGES_FILE))?;
        let (_, labels) = self.labels.finish().map_err(in_file(LABELS_FILE))?;
        Ok(Trailers {
            messages: messages.digest,
            exchanges: exchanges.digest,
            labels: labels.digest,
        })
    }
}

/// Writes `manifest` to `dir/manifest.json`: pretty JSON, a final newline.
pub fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<PathBuf, ExportError> {
    let path = dir.join(MANIFEST_FILE);
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(ExportError::Manifest)?;
    bytes.push(b'\n');
    let mut out = create(&path)?;
    out.write_all(&bytes)
        .and_then(|()| out.flush())
        .map_err(|source| ExportError::Io {
            path: path.clone(),
            source,
        })?;
    Ok(path)
}

/// Reads `dir/manifest.json`.
pub fn read_manifest(dir: &Path) -> Result<Manifest, ExportError> {
    let path = dir.join(MANIFEST_FILE);
    let bytes = std::fs::read(&path).map_err(|source| ExportError::Io {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(ExportError::Manifest)
}
