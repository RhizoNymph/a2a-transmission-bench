//! Whole sessions, taken from every row group in turn.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::helpers::parquet_rows::ParquetRows;

use crate::COLUMNS;
use crate::error::LmcacheError;
use crate::files::Segment;
use crate::schema::LmcacheRow;

/// One session's rows, with their row numbers in `file`.
#[derive(Debug, Clone)]
pub struct Session {
    pub file: String,
    pub id: String,
    pub rows: Vec<(usize, LmcacheRow)>,
}

struct Cursor {
    segment: Segment,
    rows: Option<ParquetRows<LmcacheRow>>,
    peeked: Option<(usize, LmcacheRow)>,
    started: bool,
    done: bool,
}

/// Sessions taken from each segment in turn, at most `per_file` from a
/// file. A file is sorted by session, so starting in every row group
/// spreads a sample over repositories and models. A session cut by a row
/// group boundary is read up to the boundary; the next group skips its
/// first session, which may be that one's continuation.
pub struct Sessions {
    root: PathBuf,
    cursors: Vec<Cursor>,
    per_file: Option<usize>,
    taken: BTreeMap<String, usize>,
    turn: usize,
}

impl Sessions {
    pub fn new(root: &Path, segments: Vec<Segment>, per_file: Option<usize>) -> Self {
        Self {
            root: root.to_path_buf(),
            cursors: segments
                .into_iter()
                .map(|segment| Cursor {
                    segment,
                    rows: None,
                    peeked: None,
                    started: false,
                    done: false,
                })
                .collect(),
            per_file,
            taken: BTreeMap::new(),
            turn: 0,
        }
    }

    /// The next whole session of one cursor, `None` when it has no more.
    fn read(root: &Path, cursor: &mut Cursor) -> Option<Result<Session, LmcacheError>> {
        if cursor.rows.is_none() {
            let path = root.join(&cursor.segment.file);
            match ParquetRows::open_group(&path, COLUMNS, cursor.segment.group) {
                Ok(rows) => cursor.rows = Some(rows),
                Err(error) => {
                    cursor.done = true;
                    return Some(Err(error.into()));
                }
            }
        }
        loop {
            let first = match cursor.peeked.take() {
                Some(row) => row,
                None => match cursor.rows.as_mut().and_then(Iterator::next) {
                    None => {
                        cursor.done = true;
                        return None;
                    }
                    Some(Err(error)) => {
                        cursor.done = true;
                        return Some(Err(error.into()));
                    }
                    Some(Ok(row)) => row,
                },
            };
            let id = first.1.session_id.clone();
            let mut rows = vec![first];
            loop {
                match cursor.rows.as_mut().and_then(Iterator::next) {
                    Some(Ok(row)) if row.1.session_id == id => rows.push(row),
                    Some(Ok(row)) => {
                        cursor.peeked = Some(row);
                        break;
                    }
                    Some(Err(error)) => {
                        cursor.done = true;
                        return Some(Err(error.into()));
                    }
                    None => {
                        cursor.done = true;
                        break;
                    }
                }
            }
            let skip = !cursor.started && cursor.segment.group > 0;
            cursor.started = true;
            if skip {
                if cursor.done {
                    return None;
                }
                continue;
            }
            return Some(Ok(Session {
                file: cursor.segment.file.clone(),
                id,
                rows,
            }));
        }
    }
}

impl Iterator for Sessions {
    type Item = Result<Session, LmcacheError>;

    fn next(&mut self) -> Option<Self::Item> {
        let count = self.cursors.len();
        for _ in 0..count {
            let at = self.turn % count;
            self.turn = self.turn.wrapping_add(1);
            let cursor = self.cursors.get_mut(at)?;
            if cursor.done {
                continue;
            }
            let taken = self.taken.get(&cursor.segment.file).copied().unwrap_or(0);
            if self.per_file.is_some_and(|limit| taken >= limit) {
                cursor.done = true;
                cursor.rows = None;
                continue;
            }
            match Self::read(&self.root, cursor) {
                None => {
                    cursor.rows = None;
                }
                Some(Ok(session)) => {
                    *self.taken.entry(session.file.clone()).or_insert(0) += 1;
                    if cursor.done {
                        cursor.rows = None;
                    }
                    return Some(Ok(session));
                }
                Some(Err(error)) => {
                    cursor.rows = None;
                    return Some(Err(error));
                }
            }
        }
        None
    }
}
