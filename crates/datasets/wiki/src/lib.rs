//! collusion-wiki: real agent swarms using public wikis as dead drops.
//!
//! The export (`revisions.jsonl[.gz]`, `pages.jsonl[.gz]`) records page
//! edits on public UseMod/ProWiki wikis that AI agents wrote to and read
//! from. There are no model calls, so every exchange is synthesised
//! (`Fidelity::Synthetic`) in the HTTP tool shape of [`tools`]:
//!
//! - each agent identity (the username, or `ip16:<a.b>` when it is blank)
//!   is one agent;
//! - each revision is a write: an `http_request` `POST` of the page URL
//!   whose `body` is the lines that revision inserted;
//! - when a revision's author differs from the page's previous author, a
//!   read is synthesised before the edit: a `GET` of the page whose result
//!   is the page as of the previous revision;
//! - each agent is one growing conversation; a call is the response of one
//!   exchange and its result arrives in the agent's next request ([`build`]).
//!
//! One world is one connected component of the agent–page graph
//! ([`plan`]). Labels are Heuristic tier on the page's channel: a
//! transmission from each earlier author whose lines survive into the body
//! a reader read, a reread control when the reader already read them, and
//! a relay when the reader's own edit re-inserts such a line. This is
//! crosstalk-eval's converter at 7f8a2fb, ported for parity (version 1).

pub mod attribution;
pub mod build;
pub mod error;
pub mod options;
pub mod plan;
mod read;
pub mod resource;
pub mod schema;
pub mod tally;
pub mod tools;

use std::collections::BTreeMap;
use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, WorldKey};

pub use error::WikiError;
pub use options::{Options, Selection};
use plan::WorldSpec;
use schema::{Page, Revision};
pub use tally::{FamilyStats, FamilyTally};

/// The dataset's id (crosstalk-eval's).
pub const DATASET: &str = "collusion-wiki";
/// The converter's dataset version.
pub const VERSION: u32 = 1;

/// The logical file revisions are cited from (exchange ids derive from it),
/// whether the export holds it gzipped or not.
pub const REVISIONS_FILE: &str = "revisions.jsonl";
/// The page metadata file.
pub const PAGES_FILE: &str = "pages.jsonl";

/// collusion-wiki as a stream of worlds, one per connected component.
pub struct WikiSource {
    dataset: DatasetId,
    revisions: Vec<Revision>,
    worlds: Vec<WorldSpec>,
    families: FamilyTally,
    files: FilesRead,
    filter: WorldFilter,
    pace: Pace,
}

/// Reads the export under `root` and plans its worlds under `options`, with
/// calls `pace` apart.
pub fn source(root: &Path, options: &Options, pace: Pace) -> Result<WikiSource, WikiError> {
    WikiSource::open(root, &options.selection(), pace)
}

impl WikiSource {
    /// Reads the export under `root` and plans one world per connected
    /// component of the agent–page graph, after `selection`.
    pub fn open(root: &Path, selection: &Selection, pace: Pace) -> Result<Self, WikiError> {
        let mut files = FilesRead::new();
        let families = read_page_families(root, &mut files)?;
        let mut revisions: Vec<Revision> = read::lines(
            &read::member(root, REVISIONS_FILE, &mut files)?,
            REVISIONS_FILE,
        )?;
        revisions.retain(|rev| {
            selection.keeps_page(
                &rev.wiki,
                families.get(&rev.page_id).and_then(Option::as_deref),
            )
        });
        let worlds = plan::worlds(&revisions, selection);
        let tally = FamilyTally::new(&revisions, |page| {
            families.get(page).and_then(Option::as_deref)
        });
        tracing::debug!(
            dataset = DATASET,
            revisions = revisions.len(),
            worlds = worlds.len(),
            "planned collusion-wiki worlds"
        );
        Ok(Self {
            dataset: DatasetId::new(DATASET)?,
            revisions,
            worlds,
            families: tally,
            files,
            filter: WorldFilter::All,
            pace,
        })
    }

    /// Pages, multi-author pages and revisions per `page_family`, over the
    /// selected pages (before the world-size bounds and cap).
    pub fn families(&self) -> &FamilyTally {
        &self.families
    }

    /// How many worlds were planned.
    pub fn world_count(&self) -> usize {
        self.worlds.len()
    }

    /// The planned world keys, in emission order.
    pub fn world_keys(&self) -> impl Iterator<Item = &str> {
        self.worlds.iter().map(|spec| spec.key.as_str())
    }

    /// The files read, relative to the root, for the source digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }

    fn build(&self, spec: &WorldSpec) -> Result<World, WikiError> {
        let revs: Vec<&Revision> = spec
            .revisions
            .iter()
            .filter_map(|&at| self.revisions.get(at))
            .collect();
        build::world(
            &self.dataset,
            WorldKey::new(spec.key.clone())?,
            &revs,
            self.pace,
        )
    }
}

impl TraceSource for WikiSource {
    type Error = WikiError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn select(&mut self, filter: &WorldFilter) {
        self.filter = filter.clone();
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, WikiError>> + '_ {
        self.worlds.iter().filter_map(|spec| {
            let key = match WorldKey::new(spec.key.clone()) {
                Ok(key) => key,
                Err(error) => return Some(Err(error.into())),
            };
            self.filter.keeps(&key).then(|| self.build(spec))
        })
    }
}

/// `pages.jsonl[.gz]` as a page id → task cluster map.
fn read_page_families(
    root: &Path,
    files: &mut FilesRead,
) -> Result<BTreeMap<String, Option<String>>, WikiError> {
    let pages: Vec<Page> = read::lines(&read::member(root, PAGES_FILE, files)?, PAGES_FILE)?;
    Ok(pages
        .into_iter()
        .map(|page| (page.page_id, page.page_family))
        .collect())
}
