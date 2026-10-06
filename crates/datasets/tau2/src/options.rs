//! The selection flags ct-eval's CLI had for τ²-bench, and their manifest
//! record.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

/// Which simulations to read (ct-eval's `--limit` and `--include`, no
/// defaults). Results files are sorted by name; `include` entries are
/// substrings that must all occur in a file's name. A `limit` is a number
/// of simulations spread over the files (see [`crate::files`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// At most this many simulations.
    pub limit: Option<usize>,
    pub include: Vec<String>,
}

impl Options {
    /// The manifest's `selection`: `limit` when set, and each include as
    /// `include[<n>]` (ct-eval's golden export writes them so).
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let mut settings = BTreeMap::new();
        if let Some(limit) = self.limit {
            settings.insert(
                "limit".to_owned(),
                Setting::Int(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
        }
        for (at, entry) in self.include.iter().enumerate() {
            settings.insert(format!("include[{at}]"), Setting::Text(entry.clone()));
        }
        settings
    }
}
