//! Converter notes: what the manifest records per world beyond the inputs
//! (label row counts and the converter's drop counts such as
//! `uncarried_control` or `key_group_not_a_cluster`), summed per dataset
//! and note name. The report carries them as its own `notes` section and
//! never names a world, so a holdout report keeps them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::manifest::Manifest;

/// One dataset's notes, summed over the export's worlds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetNotes {
    pub dataset: DatasetId,
    /// Worlds the manifest lists.
    pub worlds: u64,
    /// Label rows in `labels.jsonl` (`exchange_agent` rows included), summed
    /// over the worlds that record a count.
    pub labels: u64,
    /// Converter counts by name.
    pub counts: BTreeMap<String, u64>,
}

impl DatasetNotes {
    /// The notes of `manifest`'s worlds, summed.
    pub fn of(manifest: &Manifest) -> Self {
        let mut notes = Self {
            dataset: manifest.dataset.clone(),
            worlds: 0,
            labels: 0,
            counts: BTreeMap::new(),
        };
        for world in &manifest.worlds {
            notes.worlds = notes.worlds.saturating_add(1);
            notes.labels = notes.labels.saturating_add(world.labels.unwrap_or(0));
            for (name, count) in &world.notes {
                let sum = notes.counts.entry(name.clone()).or_insert(0);
                *sum = sum.saturating_add(*count);
            }
        }
        notes
    }
}
