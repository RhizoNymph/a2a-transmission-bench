//! The selection flags ct-eval had for `--dataset cipher`, with its
//! defaults, and their manifest form.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

use crate::PAIRS_PER_CIPHER;

/// Which pools are read, and how many pairs are generated from what seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// At most this many pools, in name order (`--limit`).
    pub limit: Option<usize>,
    /// Keep only pools whose stem contains one of these (`--include`,
    /// repeatable); the [default pools](crate::DEFAULT_POOLS) when empty.
    pub include: Vec<String>,
    /// Pairs per cipher (`--count`).
    pub count: usize,
    /// The generator's seed (`--corpus-seed`).
    pub seed: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            limit: None,
            include: Vec::new(),
            count: PAIRS_PER_CIPHER,
            seed: 0,
        }
    }
}

impl Options {
    /// The manifest's `selection`: `count` and `corpus_seed` always,
    /// `limit` when set, `include` (a JSON array of the needles) when not
    /// empty.
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
                Setting::Text(serde_json::Value::from(self.include.clone()).to_string()),
            );
        }
        out
    }
}

fn count_setting(value: usize) -> Setting {
    i64::try_from(value).map_or_else(|_| Setting::Text(value.to_string()), Setting::Int)
}
