//! Rows of each selected shard, taken in turn.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::helpers::parquet_rows::ParquetRows;

use crate::COLUMNS;
use crate::error::OpenSweError;
use crate::files::Shard;
use crate::schema::OpenSweRow;

/// One shard's reading state.
struct Lane {
    shard: Shard,
    open: Option<ParquetRows<OpenSweRow>>,
    taken: usize,
    done: bool,
}

/// Rows of each shard in turn: one row of the first shard, one of the
/// second, …, then the first again, at most `per_shard` from each.
pub struct RoundRobin {
    root: PathBuf,
    lanes: Vec<Lane>,
    per_shard: Option<usize>,
    cursor: usize,
    /// Shards opened since [`RoundRobin::take_opened`] was last called.
    opened: Vec<String>,
}

impl RoundRobin {
    pub fn new(root: &Path, shards: Vec<Shard>, per_shard: Option<usize>) -> Self {
        Self {
            root: root.to_path_buf(),
            lanes: shards
                .into_iter()
                .map(|shard| Lane {
                    shard,
                    open: None,
                    taken: 0,
                    done: false,
                })
                .collect(),
            per_shard,
            cursor: 0,
            opened: Vec::new(),
        }
    }

    /// The shards (relative paths) opened since the last call, in order.
    pub fn take_opened(&mut self) -> Vec<String> {
        std::mem::take(&mut self.opened)
    }

    /// The next row of the next shard with rows left; `None` when every
    /// shard is done. A shard that fails to open or read is reported once
    /// and then skipped.
    pub fn next_row(&mut self) -> Option<Result<(Shard, usize, OpenSweRow), OpenSweError>> {
        let count = self.lanes.len();
        for _ in 0..count {
            let at = self.cursor % count;
            self.cursor = self.cursor.wrapping_add(1);
            let lane = self.lanes.get_mut(at)?;
            if lane.done {
                continue;
            }
            if self.per_shard.is_some_and(|limit| lane.taken >= limit) {
                lane.done = true;
                lane.open = None;
                continue;
            }
            if lane.open.is_none() {
                let path = self.root.join(&lane.shard.relative);
                self.opened.push(lane.shard.relative.clone());
                match ParquetRows::open(&path, COLUMNS) {
                    Ok(rows) => lane.open = Some(rows),
                    Err(error) => {
                        lane.done = true;
                        return Some(Err(error.into()));
                    }
                }
            }
            match lane.open.as_mut().and_then(Iterator::next) {
                None => {
                    lane.done = true;
                    lane.open = None;
                }
                Some(Err(error)) => {
                    lane.done = true;
                    lane.open = None;
                    return Some(Err(error.into()));
                }
                Some(Ok((row, record))) => {
                    lane.taken += 1;
                    return Some(Ok((lane.shard.clone(), row, record)));
                }
            }
        }
        None
    }
}
