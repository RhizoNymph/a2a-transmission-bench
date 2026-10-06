//! The selection flags ct-eval had for `--dataset swe-splice`, with its
//! defaults, and their manifest form.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

use crate::SPLICES;

/// Which shards feed the pool, and how many splices from what seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// At most this many Open-SWE shards, in path order (`--limit`).
    pub limit: Option<usize>,
    /// Keep only shards whose relative path contains one of these
    /// (`--include`, repeatable); every shard when empty.
    pub include: Vec<String>,
    /// Splices, one world each (`--count`).
    pub count: usize,
    /// The generator's seed (`--corpus-seed`).
    pub seed: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            limit: None,
            include: Vec::new(),
            count: SPLICES,
            seed: 0,
        }
    }
}

impl Options {
    /// The manifest's `selection`: `count` and `corpus_seed` always,
    /// `limit` when set, `include` (a list of the needles, in order) when
    /// not empty.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let mut out = BTreeMap::new();
        out.insert("count".to_owned(), count_setting(self.count));
        out.insert(
            "corpus_seed".to_owned(),
            i64::try_from(self.seed)
                .map_or_else(|_| Setting::Text(self.seed.to_string()), Setting::Int),
        );
        if let Some(limit) = self.limit {
            out.insert("limit".to_owned(), count_setting(limit));
        }
        if !self.include.is_empty() {
            out.insert(
                "include".to_owned(),
                Setting::List(self.include.iter().cloned().map(Setting::Text).collect()),
            );
        }
        out
    }

    /// Trajectories in the pool: three per splice, at least 16.
    pub fn pool_size(&self) -> usize {
        self.count.saturating_mul(3).max(16)
    }
}

fn count_setting(value: usize) -> Setting {
    i64::try_from(value).map_or_else(|_| Setting::Text(value.to_string()), Setting::Int)
}
