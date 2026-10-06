//! The selection flags ct-eval had for `--dataset lmcache`, with its
//! defaults, and their manifest form.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

use crate::AGENTS_PER_WORLD;

/// Which files and sessions are read, and how they are mixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// At most this many data files, in name order (`--limit`).
    pub limit: Option<usize>,
    /// Keep only files whose relative path contains one of these
    /// (`--include`, repeatable); every file when empty.
    pub include: Vec<String>,
    /// Sessions read from each file; every session when `None` (`--count`).
    pub count: Option<usize>,
    /// Sessions per world, at least 1 (`--agents-per-world`).
    pub agents_per_world: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            limit: None,
            include: Vec::new(),
            count: None,
            agents_per_world: AGENTS_PER_WORLD,
        }
    }
}

impl Options {
    /// The manifest's `selection`: `agents_per_world` always, `limit` and
    /// `count` when set, `include` (a list of the needles, in order) when
    /// not empty.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let mut out = BTreeMap::new();
        out.insert(
            "agents_per_world".to_owned(),
            count_setting(self.agents_per_world),
        );
        if let Some(limit) = self.limit {
            out.insert("limit".to_owned(), count_setting(limit));
        }
        if let Some(count) = self.count {
            out.insert("count".to_owned(), count_setting(count));
        }
        if !self.include.is_empty() {
            out.insert(
                "include".to_owned(),
                Setting::List(self.include.iter().cloned().map(Setting::Text).collect()),
            );
        }
        out
    }
}

fn count_setting(value: usize) -> Setting {
    i64::try_from(value).map_or_else(|_| Setting::Text(value.to_string()), Setting::Int)
}
