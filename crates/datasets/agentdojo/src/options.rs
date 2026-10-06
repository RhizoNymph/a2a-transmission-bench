//! The selection flags ct-eval's CLI had for AgentDojo, and their manifest
//! record.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

/// Which runs to read (ct-eval's `--limit` and `--include`, no defaults).
///
/// `include` entries select runs (see [`crate::files`]):
/// `pipeline=<name>`, `suite=<name>`, `attack=<name>` and `task=<name>`
/// match that path component exactly; anything else is a substring of the
/// relative path. Entries with the same key are alternatives; different
/// keys (and substrings) must all hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// At most this many runs, taken from the stratified order.
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
