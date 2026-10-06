//! `session` rows (the primary agent ↔ session map) and the exchanges a
//! world holds: those of the truth's sessions.

use a2a_bench_dataset_demo_swarm::{Effect, JoinFailure, Labelled, RowKind, Side};
use a2a_bench_format::labels::Label;

use super::fixture::{self, Shape, session};
use super::{key, label};

/// Rows that name only a001 and a003 (a003's read of p2 and a001's
/// self-read): no row names a002's session.
fn without_a002() -> Vec<serde_json::Value> {
    let all = fixture::truth_rows();
    vec![fixture::header(), all[2].clone(), all[3].clone()]
}

fn agent_of(labelled: &Labelled, id: a2a_bench_format::ids::ExchangeId) -> Option<String> {
    labelled
        .world
        .agent_of(id)
        .map(|agent| agent.as_str().to_owned())
}

#[test]
fn a_session_row_brings_a_session_only_conversation_into_the_world() {
    let dir = fixture::dir("session-none");
    let written = fixture::write(&dir, &without_a002());
    let before = label(&written);
    // No row names a002's session: its exchanges are not the world's.
    assert!(before.world.exchange(written.a002[1].id).is_none());
    assert!(!before.agents.sessions.contains_key("session-a002"));
    assert_eq!(before.capture.other_sessions, 4);

    let dir = fixture::dir("session-row");
    let mut rows = without_a002();
    rows.push(session("a002", "session-a002"));
    let written = fixture::write(&dir, &rows);
    let after = label(&written);
    assert_eq!(after.resolved.sessions, 1);
    assert_eq!(after.capture.other_sessions, 0);
    for turn in &written.a002 {
        assert_eq!(agent_of(&after, turn.id).as_deref(), Some("a002"));
    }
    assert!(
        after
            .world
            .decl()
            .agents
            .iter()
            .any(|agent| agent.key == key("a002"))
    );
}

#[test]
fn a_session_row_names_a_sessions_agent_over_the_other_rows() {
    let dir = fixture::dir("session-conflict");
    let mut rows = fixture::truth_rows();
    // a002's session is a005's by its session row; the delivery rows still
    // name a002 for it.
    rows.push(session("a005", "session-a002"));
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let conflicts: Vec<_> = labelled.diagnostics.named("session_conflict").collect();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].effect, Effect::Noted);
    assert_eq!(
        conflicts[0].failure,
        JoinFailure::SessionConflict {
            session: "session-a002".to_owned(),
            agents: vec!["a002".to_owned(), "a005".to_owned()],
        }
    );
    assert_eq!(
        agent_of(&labelled, written.a002[1].id).as_deref(),
        Some("a005")
    );
    // The bench's checks refuse a label whose reader is not its reader
    // exchange's agent (ct-eval kept them): the rows read in a002's
    // session, now a005's, are dropped as invalid labels.
    let invalid: Vec<_> = labelled
        .diagnostics
        .named("invalid_label")
        .map(|entry| (entry.line, entry.row, entry.side, entry.effect))
        .collect();
    assert_eq!(
        invalid,
        [
            (
                Some(2),
                Some(RowKind::Transmission),
                Side::Row,
                Effect::Dropped
            ),
            (Some(5), Some(RowKind::Reread), Side::Row, Effect::Dropped),
            (Some(6), Some(RowKind::Miss), Side::Row, Effect::Dropped),
            (Some(6), Some(RowKind::Miss), Side::Row, Effect::Dropped),
            (Some(6), Some(RowKind::Miss), Side::Row, Effect::Dropped),
        ]
    );
}

#[test]
fn a_session_row_the_capture_lacks_is_noted() {
    let dir = fixture::dir("session-unknown");
    let mut rows = fixture::truth_rows();
    rows.push(session("a004", "session-a004"));
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let unknown: Vec<_> = labelled
        .diagnostics
        .entries
        .iter()
        .filter(|entry| entry.row == Some(RowKind::Session))
        .map(|entry| (entry.line, entry.side, entry.failure.name(), entry.effect))
        .collect();
    assert_eq!(
        unknown,
        [(Some(10), Side::Row, "unknown_session", Effect::Noted)]
    );
    assert_eq!(labelled.resolved.sessions, 1);
    // a004 is a declared agent of the world with no exchanges; a miss
    // control from it to a002 is labelled.
    assert!(
        labelled.world.labels().iter().any(
            |label| matches!(label, Label::NegativeControl(c) if c.fields().from == key("a004"))
        )
    );
}

#[test]
fn a_truth_file_without_session_rows_labels_as_with_agreeing_ones() {
    let dir = fixture::dir("session-agree-none");
    let written = fixture::write(&dir, &fixture::truth_rows());
    let plain = label(&written);
    assert_eq!(plain.resolved.sessions, 0);

    // Session rows that agree with the other rows change nothing but
    // their own count. They interleave with the other rows in event
    // order, as the swarm writes them.
    let dir = fixture::dir("session-agree-rows");
    let mut rows = fixture::truth_rows();
    rows.insert(7, session("a003", "session-a003"));
    rows.insert(3, session("a002", "session-a002"));
    rows.insert(1, session("a001", "session-a001"));
    let kinds: Vec<&str> = rows
        .iter()
        .map(|row| row["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(
        kinds,
        [
            "header",
            "session",
            "transmission",
            "transmission",
            "session",
            "self_read",
            "reread",
            "miss",
            "transmission",
            "session",
            "transmission",
            "agent_cluster"
        ]
    );
    let written = fixture::write(&dir, &rows);
    let started = label(&written);
    assert_eq!(started.resolved.sessions, 3);
    assert_eq!(started.resolved.rows, plain.resolved.rows + 3);
    let names = |labelled: &Labelled| {
        labelled
            .diagnostics
            .entries
            .iter()
            .map(|entry| (entry.failure.name(), entry.effect))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&started), names(&plain));
    // The labels are the same but for their truth line numbers.
    let shape = |labelled: &Labelled| {
        labelled
            .world
            .labels()
            .iter()
            .map(|label| match label {
                Label::ExchangeAgent(row) => format!("{} {}", row.exchange, row.agent),
                Label::Transmission(row) => {
                    let row = row.fields();
                    format!("t {} {} {}", row.from, row.to, row.reader_exchange)
                }
                Label::NegativeControl(row) => {
                    let row = row.fields();
                    format!("c {} {} {:?}", row.from, row.to, row.reason)
                }
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&started), shape(&plain));
}

// ---- the world's exchanges ----

#[test]
fn the_world_holds_the_exchanges_of_the_truths_sessions() {
    let dir = fixture::dir("exchanges-all");
    let written = fixture::write(&dir, &fixture::truth_rows());
    let labelled = label(&written);
    let all = written.a001.len() + written.a002.len() + written.a003.len();
    assert_eq!(all, 11);
    assert_eq!(labelled.resolved.exchanges, all as u64);
    assert_eq!(labelled.world.exchanges().len(), all);

    // a002's session is in the capture but in no row: its exchanges are
    // not the truth's.
    let dir = fixture::dir("exchanges-some");
    let written = fixture::write(&dir, &without_a002());
    let labelled = label(&written);
    let named = written.a001.len() + written.a003.len();
    assert_eq!(labelled.world.exchanges().len(), named);

    // A session row brings them in.
    let dir = fixture::dir("exchanges-session");
    let mut rows = without_a002();
    rows.push(session("a002", "session-a002"));
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    assert_eq!(labelled.world.exchanges().len(), all);
}

#[test]
fn an_exchange_of_another_session_is_left_out_and_counted() {
    let dir = fixture::dir("exchanges-stranger");
    let written = fixture::write_shaped(
        &dir,
        &fixture::truth_rows(),
        Shape {
            stranger: true,
            ..Shape::default()
        },
    );
    let labelled = label(&written);
    assert_eq!(written.stranger.len(), 1);
    assert!(labelled.world.exchange(written.stranger[0].id).is_none());
    assert_eq!(labelled.capture.exchanges, 12);
    assert_eq!(labelled.capture.other_sessions, 1);
    assert_eq!(labelled.world.exchanges().len(), 11);
}

#[test]
fn a_client_turn_off_its_ordinal_is_noted_and_the_ordinal_joins() {
    let dir = fixture::dir("client-turns");
    let written = fixture::write_shaped(
        &dir,
        &fixture::truth_rows(),
        Shape {
            turn_offset: 1,
            ..Shape::default()
        },
    );
    let labelled = label(&written);
    let noted: Vec<_> = labelled.diagnostics.named("client_turn_mismatch").collect();
    assert_eq!(noted.len(), 11);
    assert!(noted.iter().all(|entry| entry.effect == Effect::Noted));
    assert_eq!(
        noted[0].failure,
        JoinFailure::ClientTurnMismatch {
            session: "session-a001".to_owned(),
            exchange: written.a001[0].id,
            client_turn: 1,
            ordinal: 0,
        }
    );
    // The join is unchanged: the ordinal, not the client's turn, decides.
    assert_eq!(labelled.resolved.transmissions, 3);
    assert_eq!(labelled.diagnostics.named("turn_mismatch").count(), 1);
}
