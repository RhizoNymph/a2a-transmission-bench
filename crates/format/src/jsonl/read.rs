//! Reading a framed JSONL file, one world at a time.

use std::collections::BTreeSet;
use std::io::BufRead;

use serde::Deserialize;

use super::{FILE_DIGEST_CONTEXT, FileKind, Frame, HeaderFields, Keyed, Trailer};
use crate::ids::{Digest, WorldKey};
use crate::version::{FORMAT, Format};

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("reading line {line}: {source}")]
    Io { line: u64, source: std::io::Error },
    #[error("line {line}: {message}")]
    Json { line: u64, message: String },
    #[error("the file is empty: no header")]
    MissingHeader,
    #[error("line 1 is a {got:?} row, not the header")]
    NotHeader { got: String },
    #[error("expected a {expected} file, the header says {got:?}")]
    WrongFile { expected: &'static str, got: String },
    #[error("the file is format {got}, this reader reads {FORMAT}")]
    WrongFormat { got: Format },
    #[error("line {line}: a row before any world")]
    RowOutsideWorld { line: u64 },
    #[error("line {line}: a second header")]
    SecondHeader { line: u64 },
    #[error("world {key} appears twice")]
    DuplicateWorld { key: WorldKey },
    #[error("the file ends without a trailer: truncated")]
    Truncated,
    #[error(
        "the trailer counts {expected_worlds} worlds and {expected_rows} rows, the file holds {worlds} and {rows}"
    )]
    Counts {
        expected_worlds: u64,
        expected_rows: u64,
        worlds: u64,
        rows: u64,
    },
    #[error("the trailer's digest {expected} is not the file's {actual}")]
    Digest { expected: Digest, actual: Digest },
    #[error("line {line}: content after the trailer")]
    AfterTrailer { line: u64 },
}

/// A world's row and its rows.
#[derive(Debug, Clone)]
pub struct WorldSection<K: FileKind> {
    pub world: K::World,
    pub rows: Vec<K::Row>,
}

#[derive(Deserialize)]
struct Peek {
    kind: String,
}

const FRAME_KINDS: [&str; 3] = ["header", "world", "trailer"];

/// Reads one file. [`FileReader::open`] reads and checks the header;
/// [`FileReader::next_world`] returns world sections in order and, at the
/// end, checks the trailer's counts and digest.
pub struct FileReader<K: FileKind, R: BufRead> {
    input: R,
    header: K::Header,
    hasher: blake3::Hasher,
    line: u64,
    pending: Option<K::World>,
    seen: BTreeSet<WorldKey>,
    rows: u64,
    done: bool,
}

impl<K: FileKind, R: BufRead> FileReader<K, R> {
    pub fn open(mut input: R) -> Result<Self, ReadError> {
        let mut hasher = blake3::Hasher::new_derive_key(FILE_DIGEST_CONTEXT);
        let mut text = String::new();
        let read = input
            .read_line(&mut text)
            .map_err(|source| ReadError::Io { line: 1, source })?;
        if read == 0 {
            return Err(ReadError::MissingHeader);
        }
        hasher.update(text.as_bytes());
        let peek: Peek = parse(1, &text)?;
        if peek.kind != "header" {
            return Err(ReadError::NotHeader { got: peek.kind });
        }
        let header = match parse::<Frame<K::Header, K::World>>(1, &text)? {
            Frame::Header(header) => header,
            Frame::World(_) | Frame::Trailer(_) => return Err(ReadError::MissingHeader),
        };
        if header.format() != FORMAT {
            return Err(ReadError::WrongFormat {
                got: header.format(),
            });
        }
        if header.file() != K::NAME {
            return Err(ReadError::WrongFile {
                expected: K::NAME,
                got: header.file().to_owned(),
            });
        }
        Ok(Self {
            input,
            header,
            hasher,
            line: 1,
            pending: None,
            seen: BTreeSet::new(),
            rows: 0,
            done: false,
        })
    }

    pub fn header(&self) -> &K::Header {
        &self.header
    }

    /// The next world's section, or `None` after a valid trailer.
    pub fn next_world(&mut self) -> Result<Option<WorldSection<K>>, ReadError> {
        let mut current = self.pending.take().map(|world| WorldSection {
            world,
            rows: Vec::new(),
        });
        loop {
            let mut text = String::new();
            let read = self
                .input
                .read_line(&mut text)
                .map_err(|source| ReadError::Io {
                    line: self.line + 1,
                    source,
                })?;
            if read == 0 {
                return if self.done {
                    Ok(None)
                } else {
                    Err(ReadError::Truncated)
                };
            }
            self.line += 1;
            let line = self.line;
            if self.done {
                return Err(ReadError::AfterTrailer { line });
            }
            let peek: Peek = parse(line, &text)?;
            if !FRAME_KINDS.contains(&peek.kind.as_str()) {
                let Some(section) = current.as_mut() else {
                    return Err(ReadError::RowOutsideWorld { line });
                };
                self.hasher.update(text.as_bytes());
                section.rows.push(parse(line, &text)?);
                self.rows += 1;
                continue;
            }
            let frame: Frame<K::Header, K::World> = parse(line, &text)?;
            match frame {
                Frame::Header(_) => return Err(ReadError::SecondHeader { line }),
                Frame::World(world) => {
                    self.hasher.update(text.as_bytes());
                    if !self.seen.insert(world.key().clone()) {
                        return Err(ReadError::DuplicateWorld {
                            key: world.key().clone(),
                        });
                    }
                    match current {
                        None => {
                            current = Some(WorldSection {
                                world,
                                rows: Vec::new(),
                            })
                        }
                        Some(section) => {
                            self.pending = Some(world);
                            return Ok(Some(section));
                        }
                    }
                }
                Frame::Trailer(trailer) => {
                    self.check_trailer(&trailer)?;
                    self.done = true;
                    if current.is_some() {
                        return Ok(current);
                    }
                }
            }
        }
    }

    fn check_trailer(&self, trailer: &Trailer) -> Result<(), ReadError> {
        let worlds = u64::try_from(self.seen.len()).unwrap_or(u64::MAX);
        if trailer.worlds != worlds || trailer.rows != self.rows {
            return Err(ReadError::Counts {
                expected_worlds: trailer.worlds,
                expected_rows: trailer.rows,
                worlds,
                rows: self.rows,
            });
        }
        let actual = Digest::from_bytes(*self.hasher.finalize().as_bytes());
        if actual != trailer.digest {
            return Err(ReadError::Digest {
                expected: trailer.digest,
                actual,
            });
        }
        Ok(())
    }
}

fn parse<T: serde::de::DeserializeOwned>(line: u64, text: &str) -> Result<T, ReadError> {
    serde_json::from_str(text).map_err(|error| ReadError::Json {
        line,
        message: error.to_string(),
    })
}
