//! demo-swarm labelling over a synthetic run (`fixture`): truth parsing,
//! the join of truth rows to captured exchanges, the run window, session
//! rows, scenarios and the export. Ported from crosstalk-eval's
//! `tests/swarm_truth/` at 7f8a2fb; the scoring halves of those tests
//! (predictions, reports, gates) belong to the adapter and the scorer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod export;
mod fixture;
mod join;
mod scenario;
mod sessions;
mod truth;
mod window;

use std::io::Cursor;

use a2a_bench_dataset_demo_swarm::truth_file::{self, TruthFile, TruthFileError};
use a2a_bench_dataset_demo_swarm::{DemoSwarmSource, Labelled, Options};
use a2a_bench_format::ids::AgentKey;
use a2a_bench_format::labels::{
    ControlFields, ExemptionFields, Label, NegativeReason, TransmissionFields,
};

use fixture::Written;

pub fn key(name: &str) -> AgentKey {
    AgentKey::new(name).unwrap()
}

/// Labels the fixture's files with `options`.
pub fn label_with(written: &Written, options: &Options) -> Labelled {
    DemoSwarmSource::open(&written.inputs, options)
        .expect("the run labels")
        .into_labelled()
}

pub fn label(written: &Written) -> Labelled {
    label_with(written, &Options::default())
}

/// The fixture with its default truth, labelled.
pub fn labelled(name: &str) -> (Written, Labelled) {
    let dir = fixture::dir(name);
    let written = fixture::write(&dir, &fixture::truth_rows());
    let labelled = label(&written);
    (written, labelled)
}

pub fn read_rows(rows: &[serde_json::Value]) -> Result<TruthFile, TruthFileError> {
    let mut text = String::new();
    for row in rows {
        text.push_str(&row.to_string());
        text.push('\n');
    }
    truth_file::read(Cursor::new(text))
}

pub fn transmissions(labelled: &Labelled) -> Vec<&TransmissionFields> {
    labelled
        .world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(row) => Some(row.fields()),
            _ => None,
        })
        .collect()
}

pub fn controls(labelled: &Labelled) -> Vec<&ControlFields> {
    labelled
        .world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::NegativeControl(row) => Some(row.fields()),
            _ => None,
        })
        .collect()
}

pub fn exemptions(labelled: &Labelled) -> Vec<&ExemptionFields> {
    labelled
        .world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Exemption(row) => Some(row.fields()),
            _ => None,
        })
        .collect()
}

pub fn controls_of(labelled: &Labelled, reason: NegativeReason) -> Vec<&ControlFields> {
    controls(labelled)
        .into_iter()
        .filter(|control| control.reason == reason)
        .collect()
}

/// The transmission label made from truth line `line`.
pub fn transmission_at(labelled: &Labelled, line: usize) -> Option<&TransmissionFields> {
    let path = format!("line/{line}");
    transmissions(labelled)
        .into_iter()
        .find(|row| row.source.path() == path)
}
