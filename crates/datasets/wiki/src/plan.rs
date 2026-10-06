//! Planning worlds: one per connected component of the agent–page graph
//! (agents linked by a page both edited), after the selection's world-size
//! bounds and cap.

use std::collections::{BTreeMap, BTreeSet};

use crate::options::Selection;
use crate::schema::Revision;

/// One world's revisions: indices into the source's revisions, in the
/// world's processing order (by time, then page, then seq).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSpec {
    /// The component's lexicographically smallest page id.
    pub key: String,
    pub revisions: Vec<usize>,
    pub agents: usize,
}

/// Groups `revisions` into connected components, orders each component's
/// revisions, drops components outside the selection's agent bounds, and
/// orders worlds largest first (ties by key) before the cap.
pub fn worlds(revisions: &[Revision], selection: &Selection) -> Vec<WorldSpec> {
    let mut dsu = Dsu::default();
    // Link every identity that edited a page to the first identity seen on it.
    let mut first_on_page: BTreeMap<&str, usize> = BTreeMap::new();
    for rev in revisions {
        let node = dsu.node(rev.identity());
        match first_on_page.get(rev.page_id.as_str()) {
            Some(&anchor) => dsu.union(anchor, node),
            None => {
                first_on_page.insert(&rev.page_id, node);
            }
        }
    }
    // Group revision indices by their identity's component root.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (at, rev) in revisions.iter().enumerate() {
        let node = dsu.node(rev.identity());
        let root = dsu.find(node);
        groups.entry(root).or_default().push(at);
    }
    let mut specs = Vec::new();
    for mut revs in groups.into_values() {
        let agents: BTreeSet<String> = revs.iter().map(|&at| revisions[at].identity()).collect();
        if selection.min_agents.is_some_and(|min| agents.len() < min)
            || selection.max_agents.is_some_and(|max| agents.len() > max)
        {
            continue;
        }
        revs.sort_by(|&a, &b| {
            let (ra, rb) = (&revisions[a], &revisions[b]);
            (&ra.time, &ra.page_id, ra.seq).cmp(&(&rb.time, &rb.page_id, rb.seq))
        });
        // Pages never cross components, so the smallest page id is unique
        // to the component.
        let key = revs
            .iter()
            .map(|&at| revisions[at].page_id.as_str())
            .min()
            .unwrap_or("")
            .to_owned();
        specs.push(WorldSpec {
            key,
            revisions: revs,
            agents: agents.len(),
        });
    }
    specs.sort_by(|a, b| b.agents.cmp(&a.agents).then_with(|| a.key.cmp(&b.key)));
    if let Some(limit) = selection.limit {
        specs.truncate(limit);
    }
    specs
}

/// A disjoint-set forest over identity strings. The node numbering and the
/// union direction are crosstalk-eval's, so component roots (and so the
/// order of equal-sized worlds before sorting) are too.
#[derive(Default)]
struct Dsu {
    index: BTreeMap<String, usize>,
    parent: Vec<usize>,
}

impl Dsu {
    fn node(&mut self, identity: String) -> usize {
        if let Some(&at) = self.index.get(&identity) {
            return at;
        }
        let at = self.parent.len();
        self.parent.push(at);
        self.index.insert(identity, at);
        at
    }

    fn find(&mut self, mut at: usize) -> usize {
        while self.parent[at] != at {
            self.parent[at] = self.parent[self.parent[at]];
            at = self.parent[at];
        }
        at
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}
