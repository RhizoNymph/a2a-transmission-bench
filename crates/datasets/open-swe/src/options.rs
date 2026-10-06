//! The selection flags ct-eval had for `--dataset open-swe`, with its
//! defaults, and their manifest form.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

use crate::AGENTS_PER_WORLD;

/// Which shards and rows are read, and how they are mixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// At most this many shards, in path order (`--limit`).
    pub limit: Option<usize>,
    /// Keep only shards whose relative path contains one of these
    /// (`--include`, repeatable); every shard when empty.
    pub include: Vec<String>,
    /// Rows read from each shard; every row when `None` (`--count`).
    pub count: Option<usize>,
    /// Trajectories per world, at least 1 (`--agents-per-world`).
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
    /// `count` when set, `include` (a JSON array of the needles) when not
    /// empty.
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
            out.insert("include".to_owned(), include_setting(&self.include));
        }
        out
    }
}

/// A count as an integer setting (its decimal text past `i64::MAX`).
pub fn count_setting(value: usize) -> Setting {
    i64::try_from(value).map_or_else(|_| Setting::Text(value.to_string()), Setting::Int)
}

/// Include needles as one text setting: their JSON array.
pub fn include_setting(needles: &[String]) -> Setting {
    Setting::Text(serde_json::Value::from(needles.to_vec()).to_string())
}
