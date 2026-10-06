//! Counts over a real swarm run's truth file, for the real-data
//! comparison with ct-eval (`docs/features/dataset-demo-swarm.md`).
//! Ignored by default: run with
//! `DEMO_SWARM_TRUTH=<run dir>/truth.jsonl cargo test -p a2a-bench-dataset-demo-swarm --test real_truth -- --ignored --nocapture`.
//! Prints counts only, never row text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;

use a2a_bench_dataset_demo_swarm::truth_file::{self, DeliveryKind, Row};
use a2a_bench_dataset_demo_swarm::{Margins, RunWindow};

#[test]
#[ignore = "reads a real run named by DEMO_SWARM_TRUTH"]
fn count_a_real_truth_file() {
    let Ok(path) = std::env::var("DEMO_SWARM_TRUTH") else {
        panic!("set DEMO_SWARM_TRUTH to a truth.jsonl");
    };
    let truth = truth_file::read(BufReader::new(File::open(&path).unwrap())).unwrap();
    let (mut sessions, mut transmissions, mut self_reads, mut rereads) = (0, 0, 0, 0);
    let (mut misses, mut unattributed, mut key_groups, mut shared_groups) = (0, 0, 0, 0);
    let mut agents = BTreeSet::new();
    for numbered in &truth.rows {
        match &numbered.row {
            Row::Session(row) => {
                sessions += 1;
                agents.insert(row.agent.clone());
            }
            Row::Delivery { kind, row } => {
                agents.insert(row.writer.clone());
                agents.insert(row.reader.clone());
                match kind {
                    DeliveryKind::Transmission => transmissions += 1,
                    DeliveryKind::SelfRead => self_reads += 1,
                    DeliveryKind::Reread => rereads += 1,
                }
            }
            Row::Miss(row) => {
                misses += 1;
                agents.insert(row.reader.clone());
            }
            Row::Unattributed(row) => {
                unattributed += 1;
                agents.insert(row.reader.clone());
            }
            Row::Cluster(row) => {
                key_groups += 1;
                agents.extend(row.agents.iter().cloned());
                if row.agents.iter().collect::<BTreeSet<_>>().len() >= 2 {
                    shared_groups += 1;
                }
            }
        }
    }
    let window = RunWindow::of(&truth, Margins::default());
    println!(
        "scenario={} rows={} sessions={sessions} transmissions={transmissions} self_reads={self_reads} \
         rereads={rereads} misses={misses} miss_controls={} unattributed={unattributed} \
         key_groups={key_groups} shared_key_groups={shared_groups} agents={} \
         window={}..{}",
        truth.header.scenario().as_str(),
        truth.rows.len(),
        misses * (agents.len().saturating_sub(1)),
        agents.len(),
        window.start_unix_ms,
        window.end_unix_ms,
    );
}
