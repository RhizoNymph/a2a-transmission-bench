//! A dataset as a stream of worlds.
//!
//! A [`TraceSource`] yields one finished [`World`] at a time, so no dataset
//! is ever held whole: memory is bounded by the largest world. A world is
//! the unit because its exchanges and labels come from one parse of the
//! same records.

use std::collections::BTreeSet;

use a2a_bench_format::ids::{DatasetId, WorldKey};

use crate::world::World;

/// Which worlds an export keeps, by key. A source may use it to skip
/// parsing worlds it will not keep ([`TraceSource::select`]); the export
/// applies it to every world whatever the source does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldFilter {
    /// Every world.
    All,
    /// Only these.
    Only(BTreeSet<WorldKey>),
    /// All but these.
    Except(BTreeSet<WorldKey>),
}

impl WorldFilter {
    pub fn keeps(&self, key: &WorldKey) -> bool {
        match self {
            Self::All => true,
            Self::Only(keys) => keys.contains(key),
            Self::Except(keys) => !keys.contains(key),
        }
    }
}

/// A dataset as a stream of worlds.
pub trait TraceSource {
    /// Why a world could not be produced. The export logs it, records it
    /// as a failure and goes on with the next world.
    type Error: std::error::Error;

    fn dataset(&self) -> &DatasetId;

    /// A hint of the worlds the caller will keep, given before
    /// [`TraceSource::worlds`]. A source whose world keys are known before
    /// parsing (a file stem) may skip the others; the default ignores it.
    fn select(&mut self, filter: &WorldFilter) {
        let _ = filter;
    }

    /// The worlds, one at a time, in a deterministic order. An error ends
    /// nothing: the next item is the next world.
    fn worlds(&mut self) -> impl Iterator<Item = Result<World, Self::Error>> + '_;
}

/// A source over worlds already built (tests, small fixtures), each yielded
/// once.
pub struct InMemory<E = std::convert::Infallible> {
    dataset: DatasetId,
    worlds: Vec<Result<World, E>>,
}

impl InMemory {
    pub fn new(dataset: DatasetId, worlds: Vec<World>) -> Self {
        Self {
            dataset,
            worlds: worlds.into_iter().map(Ok).collect(),
        }
    }
}

impl<E> InMemory<E> {
    /// A source that yields `worlds` as they are, failures included.
    pub fn with_failures(dataset: DatasetId, worlds: Vec<Result<World, E>>) -> Self {
        Self { dataset, worlds }
    }
}

impl<E: std::error::Error> TraceSource for InMemory<E> {
    type Error = E;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, E>> + '_ {
        self.worlds.drain(..)
    }
}
