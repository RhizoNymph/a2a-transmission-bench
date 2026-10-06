//! Writing a framed JSONL file.

use std::collections::BTreeSet;
use std::io::Write;
use std::marker::PhantomData;

use super::{FILE_DIGEST_CONTEXT, FileKind, Frame, HeaderFields, Keyed, Trailer};
use crate::ids::{Digest, WorldKey};
use crate::version::FORMAT;

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("writing: {0}")]
    Io(#[from] std::io::Error),
    #[error("encoding a line: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("a {kind} header names file {got:?}")]
    WrongFile { kind: &'static str, got: String },
    #[error("the header names format {got}, this writer writes {FORMAT}")]
    WrongFormat { got: crate::version::Format },
    #[error("a row was written before any world")]
    RowOutsideWorld,
    #[error("world {0} was written twice")]
    DuplicateWorld(WorldKey),
}

/// Writes one file: the header on creation, then world sections, then the
/// trailer on [`FileWriter::finish`]. A writer dropped unfinished leaves a
/// file without a trailer, which every reader refuses.
pub struct FileWriter<K: FileKind, W: Write> {
    out: W,
    hasher: blake3::Hasher,
    worlds: BTreeSet<WorldKey>,
    rows: u64,
    kind: PhantomData<K>,
}

impl<K: FileKind, W: Write> FileWriter<K, W> {
    pub fn new(out: W, header: &K::Header) -> Result<Self, WriteError> {
        if header.file() != K::NAME {
            return Err(WriteError::WrongFile {
                kind: K::NAME,
                got: header.file().to_owned(),
            });
        }
        if header.format() != FORMAT {
            return Err(WriteError::WrongFormat {
                got: header.format(),
            });
        }
        let mut writer = Self {
            out,
            hasher: blake3::Hasher::new_derive_key(FILE_DIGEST_CONTEXT),
            worlds: BTreeSet::new(),
            rows: 0,
            kind: PhantomData,
        };
        writer.line(&Frame::<&K::Header, &K::World>::Header(header))?;
        Ok(writer)
    }

    fn line(&mut self, value: &impl serde::Serialize) -> Result<(), WriteError> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        self.hasher.update(&bytes);
        self.out.write_all(&bytes)?;
        Ok(())
    }

    /// Starts a world's section.
    pub fn world(&mut self, world: &K::World) -> Result<(), WriteError> {
        if !self.worlds.insert(world.key().clone()) {
            return Err(WriteError::DuplicateWorld(world.key().clone()));
        }
        self.line(&Frame::<&K::Header, &K::World>::World(world))
    }

    /// A row of the current world.
    pub fn row(&mut self, row: &K::Row) -> Result<(), WriteError> {
        if self.worlds.is_empty() {
            return Err(WriteError::RowOutsideWorld);
        }
        self.rows += 1;
        self.line(row)
    }

    /// Writes the trailer and returns the output and the trailer.
    pub fn finish(mut self) -> Result<(W, Trailer), WriteError> {
        let trailer = Trailer {
            worlds: u64::try_from(self.worlds.len()).unwrap_or(u64::MAX),
            rows: self.rows,
            digest: Digest::from_bytes(*self.hasher.finalize().as_bytes()),
        };
        self.line(&Frame::<&K::Header, &K::World>::Trailer(trailer.clone()))?;
        self.out.flush()?;
        Ok((self.out, trailer))
    }
}
