//! LMCache agentic traces as a background corpus, on synthetic Parquet files
//! shaped like the dataset (`tests/fixtures/lmcache`, rebuilt by its
//! `make.sql`). The first file has two row groups; a session crosses the
//! boundary between them. Ported from crosstalk-eval's `tests/lmcache.rs`
//! at 7f8a2fb.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::EPOCH_MICROS;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::{StopReason, World};
use a2a_bench_dataset_lmcache::{
    AGENTS_PER_WORLD, DATASET, Options, Segment, Session, Sessions, VERSION, calls, discover,
    group, segments, source,
};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::labels::{Label, NegativeReason, Tier};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::{AssistantPart, Body, ToolPart};

const FIRST: &str = "data/train-00000-of-00002.parquet";
const SECOND: &str = "data/train-00001-of-00002.parquet";
const A: &str = "swebench__acme__acme-1__claude";
const B: &str = "swebench__acme__acme-2__minimax";
const C: &str = "gaia__task-9__claude";
const D: &str = "swebench__beta__beta-7__deepseek";
const E: &str = "wildclaw__job-3__claude";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lmcache")
}

fn sessions(per_file: Option<usize>) -> Vec<Session> {
    let files = discover(&root(), None, &[]).unwrap();
    let segments = segments(&root(), &files).unwrap();
    Sessions::new(&root(), segments, per_file)
        .map(|session| session.unwrap())
        .collect()
}

fn session(id: &str) -> Session {
    sessions(None)
        .into_iter()
        .find(|session| session.id == id)
        .unwrap_or_else(|| panic!("no session {id}"))
}

fn worlds(agents_per_world: usize) -> Vec<World> {
    let options = Options {
        agents_per_world,
        ..Options::default()
    };
    let mut source = source(&root(), &options).unwrap();
    source.worlds().map(|world| world.unwrap()).collect()
}

#[test]
fn the_dataset_is_ct_evals() {
    assert_eq!(DATASET, "lmcache");
    assert_eq!(VERSION, 1);
    assert_eq!(AGENTS_PER_WORLD, 16);
    assert_eq!(
        Options::default().settings(),
        BTreeMap::from([("agents_per_world".to_owned(), Setting::Int(16))])
    );
    let set = Options {
        limit: Some(5),
        count: Some(16),
        ..Options::default()
    };
    assert_eq!(set.settings().get("count"), Some(&Setting::Int(16)));
    assert_eq!(set.settings().get("limit"), Some(&Setting::Int(5)));
    let listed = Options {
        include: vec!["b".into(), "a".into()],
        ..Options::default()
    };
    assert_eq!(
        listed.settings().get("include"),
        Some(&Setting::List(vec![
            Setting::Text("b".into()),
            Setting::Text("a".into())
        ]))
    );
}

#[test]
fn files_and_row_groups_are_interleaved() {
    let files = discover(&root(), None, &[]).unwrap();
    assert_eq!(files, vec![FIRST.to_owned(), SECOND.to_owned()]);
    let found = segments(&root(), &files).unwrap();
    let segment = |file: &str, group| Segment {
        file: file.into(),
        group,
    };
    assert_eq!(
        found,
        vec![segment(FIRST, 0), segment(SECOND, 0), segment(FIRST, 1)]
    );
    let limited = discover(&root(), Some(1), &[]).unwrap();
    assert_eq!(limited, vec![FIRST.to_owned()]);
    let included = discover(&root(), None, &["00001".into()]).unwrap();
    assert_eq!(included, vec![SECOND.to_owned()]);
    assert!(discover(Path::new("/nonexistent"), None, &[]).is_err());
}

#[test]
fn sessions_come_from_each_segment_in_turn() {
    let all = sessions(None);
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    // The second row group opens with the tail of B, which is skipped there.
    assert_eq!(ids, vec![A, D, C, B, E]);
    let rows = |s: &Session| s.rows.iter().map(|(row, _)| *row).collect::<Vec<_>>();
    assert_eq!(rows(&all[0]), vec![0, 1, 2]);
    assert_eq!(rows(&all[2]), vec![7, 8]);
    assert_eq!(rows(&all[3]), vec![3, 4, 5]);
    assert_eq!(all[2].file, FIRST);
    let capped: Vec<String> = sessions(Some(1)).into_iter().map(|s| s.id).collect();
    assert_eq!(capped, vec![A.to_owned(), D.to_owned()]);
}

#[test]
fn a_call_receives_what_the_next_request_appends() {
    let found = calls(&session(A)).unwrap();
    // Three requests: the last has no recorded response.
    assert_eq!(found.len(), 2);
    let Body::Assistant(parts) = found[0].response.body() else {
        panic!("the response is the appended assistant message");
    };
    assert!(
        matches!(&parts[0], AssistantPart::Text { text } if text == "Looking at the frobnicator.")
    );
    assert!(matches!(&parts[1], AssistantPart::ToolCall(c) if c.call_id == "toolu_fixture_a1"));
    assert_eq!(found[0].stop, StopReason::ToolUse);
    assert_eq!(found[1].stop, StopReason::EndTurn);
    assert_eq!(found[0].request.len(), 2);
    assert_eq!(found[1].request.len(), 4);
    // The response is echoed unchanged in the next request.
    assert_eq!(found[1].request[2], found[0].response);
    assert!(found.iter().all(|c| c.fidelity == Fidelity::Reconstructed));
    assert_eq!(found[0].source.path(), "/rows/0");
    assert_eq!(found[1].source.path(), "/rows/1");
    assert_eq!(found[0].source.file(), FIRST);
}

#[test]
fn the_clock_sums_pre_gaps() {
    let found = calls(&session(A)).unwrap();
    assert_eq!(found[0].at.as_micros(), EPOCH_MICROS);
    assert_eq!(found[1].at.as_micros(), EPOCH_MICROS + 500_000);
    let d = calls(&session(D)).unwrap();
    assert_eq!(d[1].at.as_micros(), EPOCH_MICROS + 2_000_000);
}

#[test]
fn a_rewritten_history_makes_the_call_synthetic() {
    let found = calls(&session(B)).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].fidelity, Fidelity::Reconstructed);
    assert_eq!(found[1].fidelity, Fidelity::Synthetic);
}

#[test]
fn tool_messages_keep_their_recorded_call_ids() {
    let found = calls(&session(D)).unwrap();
    let ids: Vec<String> = found[1]
        .request
        .iter()
        .filter_map(|m| match m.body() {
            Body::Tool(results) => results
                .first()
                .map(|ToolPart::ToolResult(r)| r.call_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec!["call_fixture_d1".to_owned()]);
    assert!(calls(&session(E)).unwrap().is_empty());
}

#[test]
fn sessions_are_grouped_by_repository_or_task() {
    assert_eq!(group(A), "swebench/acme");
    assert_eq!(group(B), "swebench/acme");
    assert_eq!(group(C), "gaia/task-9");
    assert_eq!(group("loose"), "loose");
}

#[test]
fn sessions_mix_into_background_worlds() {
    let mixed = worlds(16);
    assert_eq!(mixed.len(), 1);
    let world = &mixed[0];
    assert_eq!(world.key().as_str(), "mix-00000");
    assert_eq!(world.decl().agents.len(), 5);
    assert_eq!(
        world.coverage(),
        Coverage::Complete {
            tier: Tier::Construction
        }
    );
    // A 2, B 2, C 1, D 2, E 0 calls.
    assert_eq!(world.exchanges().len(), 7);
    for exchange in world.exchanges() {
        // Every call has its response (ct-eval: `Completed`).
        assert_eq!(exchange.response.messages.len(), 1);
        assert!(exchange.response.stop.is_some());
    }
    let controls: Vec<_> = world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::NegativeControl(c) => Some(c.fields()),
            _ => None,
        })
        .collect();
    let shared = controls
        .iter()
        .filter(|c| c.reason == NegativeReason::SharedSource)
        .count();
    // A and B both work on swebench/acme: each reads the other twice.
    assert_eq!(shared, 4);
    assert_eq!(controls.len(), 7 * 4);
    // Besides the controls, one exchange_agent row per exchange.
    assert_eq!(world.labels().len(), 7 * 4 + 7);
    assert_eq!(worlds(2).len(), 3);
}

#[test]
fn every_selected_file_is_read() {
    let mut source = source(&root(), &Options::default()).unwrap();
    assert_eq!(source.files(), [FIRST.to_owned(), SECOND.to_owned()]);
    assert_eq!(source.worlds().count(), 1);
    assert_eq!(source.files_read().len(), 2);
}

#[test]
fn a_rerun_is_identical() {
    let summary = |worlds: Vec<World>| -> Vec<_> {
        worlds
            .iter()
            .map(|w| (w.key().clone(), w.exchanges().to_vec(), w.labels().to_vec()))
            .collect()
    };
    assert_eq!(summary(worlds(2)), summary(worlds(2)));
}
