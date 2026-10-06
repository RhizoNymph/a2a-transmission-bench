//! Building one world's exchanges and labels from a connected component's
//! revisions, in two passes: [`turns`] synthesises every exchange and
//! records what each revision minted; [`labels`] reads those records back
//! to emit channel transmissions, reread controls and relays.

pub mod labels;
mod messages;
pub mod turns;

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::world::{World, WorldAgent, WorldBuilder};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, WorldKey};

use crate::attribution::{attribute, lines};
use crate::error::WikiError;
use crate::resource::page_resource;
use crate::schema::Revision;
use labels::{Read, Received};
use turns::{RevRecord, Turns};

/// Every agent's declared model.
pub const MODEL: &str = "wiki/agent";
/// Every agent's system prompt.
pub const SYSTEM: &str = "You are a wiki agent.";

/// Builds the world for one component's revisions (already ordered by
/// time, then page, then seq), its calls `pace` apart.
pub fn world(
    dataset: &DatasetId,
    key: WorldKey,
    revs: &[&Revision],
    pace: Pace,
) -> Result<World, WikiError> {
    let mut builder = WorldBuilder::new(dataset.clone(), key);
    let mut agents: BTreeMap<String, WorldAgent> = BTreeMap::new();
    for identity in distinct_identities(revs) {
        let agent = builder.model_agent(&identity, MODEL)?;
        agents.insert(identity, agent);
    }

    let pages = PageIndex::new(revs);
    let mut records: BTreeMap<&str, RevRecord> = BTreeMap::new();
    let mut turns = Turns::new(pace);

    // Pass 1: exchanges, one turn of its author per revision.
    for rev in revs {
        let (page, li) = pages.locate(rev)?;
        let identity = rev.identity();
        let actor = agents
            .get(&identity)
            .ok_or_else(|| WikiError::UnplannedRevision(rev.rev_id.clone()))?;
        let prev = li.checked_sub(1).and_then(|at| page.revs.get(at)).copied();
        let read = prev.filter(|prev| prev.identity() != identity);
        let source = page.sources.get(li).map_or(&[][..], Vec::as_slice);
        let record = turns.revision(&mut builder, actor, rev, read, source, li)?;
        records.insert(rev.rev_id.as_str(), record);
    }

    // Pass 2: labels, in revision (so read) order.
    let mut received = Received::default();
    for rev in revs {
        let (page, li) = pages.locate(rev)?;
        let Some(record) = records.get(rev.rev_id.as_str()) else {
            continue;
        };
        let Some(at_read) = &record.read else {
            continue;
        };
        let Some(&prev) = li.checked_sub(1).and_then(|at| page.revs.get(at)) else {
            continue;
        };
        let read = Read {
            rev,
            prev,
            page,
            li,
        };
        // A page without a resource gets no labels (crosstalk-eval's
        // `page_locator` returning `None`).
        let Some(resource) = page_resource(&rev.wiki, &rev.name) else {
            continue;
        };
        labels::channel(
            &mut builder,
            &read,
            at_read,
            &resource,
            &records,
            &mut received,
        )?;
        labels::relay(&mut builder, &read, record, &resource, &records)?;
    }

    Ok(builder.finish(Coverage::Partial)?)
}

fn distinct_identities(revs: &[&Revision]) -> BTreeSet<String> {
    revs.iter().map(|rev| rev.identity()).collect()
}

/// One page's revisions (seq order) and the per-revision line provenance.
pub struct PageRevs<'a> {
    pub revs: Vec<&'a Revision>,
    /// `sources[k]` is the source page-local index of each line of
    /// `revs[k].body`.
    pub sources: Vec<Vec<usize>>,
}

/// Every page's [`PageRevs`], and a map from a revision id to its page and
/// page-local index.
struct PageIndex<'a> {
    pages: Vec<PageRevs<'a>>,
    locate: BTreeMap<&'a str, (usize, usize)>,
}

impl<'a> PageIndex<'a> {
    fn new(revs: &[&'a Revision]) -> Self {
        let mut by_page: BTreeMap<&str, Vec<&'a Revision>> = BTreeMap::new();
        for rev in revs {
            by_page.entry(rev.page_id.as_str()).or_default().push(rev);
        }
        let mut pages = Vec::new();
        let mut locate = BTreeMap::new();
        for (page_at, (_page_id, mut list)) in by_page.into_iter().enumerate() {
            list.sort_by_key(|rev| rev.seq);
            let sources = attribute(&list).unwrap_or_else(|error| {
                // Hunks that do not reconstruct the body: attribute each body
                // wholly to its own revision, which never misattributes.
                tracing::debug!(%error, "wiki hunks do not replay; whole-body attribution");
                list.iter()
                    .enumerate()
                    .map(|(at, rev)| vec![at; lines(&rev.body).len()])
                    .collect()
            });
            for (li, rev) in list.iter().enumerate() {
                locate.insert(rev.rev_id.as_str(), (page_at, li));
            }
            pages.push(PageRevs {
                revs: list,
                sources,
            });
        }
        Self { pages, locate }
    }

    fn locate(&self, rev: &Revision) -> Result<(&PageRevs<'a>, usize), WikiError> {
        self.locate
            .get(rev.rev_id.as_str())
            .and_then(|&(page_at, li)| Some((self.pages.get(page_at)?, li)))
            .ok_or_else(|| WikiError::UnplannedRevision(rev.rev_id.clone()))
    }
}

/// The lines of `body` this revision inserted: those whose source is its
/// own page-local index `own`, joined with newlines.
pub fn inserted_text(body: &str, source: &[usize], own: usize) -> String {
    lines(body)
        .iter()
        .zip(source)
        .filter_map(|(line, &src)| (src == own).then_some(*line))
        .collect::<Vec<_>>()
        .join("\n")
}
