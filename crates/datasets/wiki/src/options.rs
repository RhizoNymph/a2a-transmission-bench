//! The selection flags crosstalk-eval's CLI had for collusion-wiki, and the
//! selection they resolve to.

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

/// ct-eval's wiki flags (`--family`, `--wiki`, `--min-agents`,
/// `--max-agents`, `--limit`, `--demo`), with its defaults (none set).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Keep only pages in these task clusters (any when empty).
    pub families: Vec<String>,
    /// Keep only pages on these wikis (any when empty).
    pub wikis: Vec<String>,
    /// Keep only worlds with at least this many agents.
    pub min_agents: Option<usize>,
    /// Drop worlds with more than this many agents.
    pub max_agents: Option<usize>,
    /// Emit at most this many worlds (the largest first).
    pub limit: Option<usize>,
    /// The demo subset ([`Selection::demo`]); overrides every other option.
    pub demo: bool,
}

impl Options {
    /// The selection these options make: the demo subset when `demo` is
    /// set, whatever else is, as ct-eval's `--demo` did.
    pub fn selection(&self) -> Selection {
        if self.demo {
            Selection::demo()
        } else {
            Selection {
                families: self.families.clone(),
                wikis: self.wikis.clone(),
                min_agents: self.min_agents,
                max_agents: self.max_agents,
                limit: self.limit,
            }
        }
    }

    /// The manifest's `selection`: `demo`, and the resolved selection's
    /// bounds and filters that are set (lists joined with `,`).
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let selection = self.selection();
        let mut out = BTreeMap::from([("demo".to_owned(), Setting::Bool(self.demo))]);
        let int = |value: usize| Setting::Int(i64::try_from(value).unwrap_or(i64::MAX));
        for (key, value) in [
            ("min_agents", selection.min_agents),
            ("max_agents", selection.max_agents),
            ("limit", selection.limit),
        ] {
            if let Some(value) = value {
                out.insert(key.to_owned(), int(value));
            }
        }
        for (key, list) in [("family", &selection.families), ("wiki", &selection.wikis)] {
            if !list.is_empty() {
                out.insert(key.to_owned(), Setting::Text(list.join(",")));
            }
        }
        out
    }
}

/// Which worlds a run reads (ct-eval's `WikiSelection`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub families: Vec<String>,
    pub wikis: Vec<String>,
    pub min_agents: Option<usize>,
    pub max_agents: Option<usize>,
    pub limit: Option<usize>,
}

impl Selection {
    /// The demo subset: the five largest relay-coordination worlds of 2 to
    /// 12 agents.
    pub fn demo() -> Self {
        Self {
            families: vec!["relay-coordination".to_owned()],
            wikis: Vec::new(),
            min_agents: Some(2),
            max_agents: Some(12),
            limit: Some(5),
        }
    }

    /// Whether a page on `wiki` in `family` is kept.
    pub fn keeps_page(&self, wiki: &str, family: Option<&str>) -> bool {
        let family_ok = self.families.is_empty()
            || family.is_some_and(|f| self.families.iter().any(|want| want == f));
        let wiki_ok = self.wikis.is_empty() || self.wikis.iter().any(|want| want == wiki);
        family_ok && wiki_ok
    }
}
