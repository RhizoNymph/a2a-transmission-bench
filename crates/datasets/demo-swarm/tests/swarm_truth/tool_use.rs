//! The join by tool use id: a row's reader is the first exchange of its
//! session whose request carries the tool result, and its writer the
//! exchange whose response holds the `PUT`, whatever turns the truth
//! states. Turns are only cross-checked (`turn_mismatch`).

use a2a_bench_dataset_demo_swarm::{Effect, JoinFailure, RowKind, Side};
use a2a_bench_format::ids::ExchangeId;
use a2a_bench_format::labels::NegativeReason;
use serde_json::json;

use super::fixture::{self, P1, P2, Shape};
use super::{controls_of, label, transmission_at};

fn a001(turn: u32, call: &str) -> (&'static str, &'static str, u32, &str) {
    ("a001", "session-a001", turn, call)
}

fn a002(turn: u32, call: &str) -> (&'static str, &'static str, u32, &str) {
    ("a002", "session-a002", turn, call)
}

fn a003(turn: u32, call: &str) -> (&'static str, &'static str, u32, &str) {
    ("a003", "session-a003", turn, call)
}

fn miss(turn: u32) -> serde_json::Value {
    json!({"kind": "miss", "world": fixture::WORLD, "reader": "a002", "reader_key_group": 1,
        "page": "p9", "reader_session": "session-a002", "reader_turn": turn,
        "reader_tool_use_id": "toolu_m",
        "read_tool": {"name": "http_request", "input": {"method": "GET", "url": fixture::url("p9")}},
        "at_ms": 101_000, "at_unix_ms": fixture::START_MS + 101_000})
}

/// The fixture's run as the truth states it when a002's turn counter is
/// `skew` ahead of the captured exchanges (a request refused before the
/// gateway, or read but not captured): a001 and a003 at their own turns.
fn rows_with_a002_ahead(skew: u32) -> Vec<serde_json::Value> {
    vec![
        fixture::header(),
        // Line 2: a001's p1 reaches a002 (captured at a002's turn 1).
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(1 + skew, "toolu_r1"),
            "p1",
            P1,
        ),
        // Line 3: a001's p2 reaches a003.
        fixture::delivery(
            "transmission",
            a001(1, "toolu_w2"),
            a003(1, "toolu_r3"),
            "p2",
            P2,
        ),
        // Line 4: a001 reads its own page.
        fixture::delivery(
            "self_read",
            a001(0, "toolu_w1"),
            a001(3, "toolu_self"),
            "p1",
            P1,
        ),
        // Line 5: a002 reads p1 again (captured at turn 2).
        fixture::delivery(
            "reread",
            a001(0, "toolu_w1"),
            a002(2 + skew, "toolu_r2"),
            "p1",
            P1,
        ),
        // Line 6: a002's miss (captured at turn 3, its session's last).
        miss(3 + skew),
        // Line 7: a001's p1 reaches a003 (captured at a003's turn 2).
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a003(2, "toolu_r4"),
            "p1",
            P1,
        ),
    ]
}

/// Every turn_mismatch as (line, side, stated turn, found turn, exchange).
fn turn_mismatches(
    labelled: &a2a_bench_dataset_demo_swarm::Labelled,
) -> Vec<(Option<usize>, Side, u32, u32, ExchangeId)> {
    labelled
        .diagnostics
        .entries
        .iter()
        .filter_map(|entry| match &entry.failure {
            JoinFailure::TurnMismatch {
                turn,
                found_turn,
                exchange,
                ..
            } => {
                assert_eq!(entry.effect, Effect::Kept, "{entry:?}");
                Some((entry.line, entry.side, *turn, *found_turn, *exchange))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_session_whose_turns_run_one_ahead_is_labelled_at_the_first_carrying_exchanges() {
    let dir = fixture::dir("tool-use-ahead");
    let ahead = fixture::write(&dir, &rows_with_a002_ahead(1));
    let labelled = label(&ahead);

    // The stale turn's exchange carries the same tool result as history:
    // the label must land on the first exchange carrying it, not there.
    let line2 = transmission_at(&labelled, 2).expect("line 2 is labelled");
    assert_eq!(line2.reader_exchange, ahead.a002[1].id);
    assert_eq!(line2.content.at.exchange, ahead.a002[1].id);
    assert_eq!(line2.sender_exchange, Some(ahead.a001[0].id));
    let rereads = controls_of(&labelled, NegativeReason::Reread);
    assert_eq!(rereads.len(), 1);
    assert_eq!(rereads[0].reader_exchange, Some(ahead.a002[2].id));
    let misses = controls_of(&labelled, NegativeReason::Miss);
    assert_eq!(misses.len(), 2);
    assert!(
        misses
            .iter()
            .all(|control| control.reader_exchange == Some(ahead.a002[3].id))
    );

    // Nothing is dropped; each skewed row is reported with both turns.
    assert_eq!(labelled.resolved.dropped, 0);
    assert_eq!(labelled.resolved.transmissions, 3);
    assert_eq!(labelled.resolved.rereads, 1);
    assert_eq!(labelled.resolved.misses, 1);
    assert_eq!(
        turn_mismatches(&labelled),
        [
            (Some(2), Side::Reader, 2, 1, ahead.a002[1].id),
            (Some(5), Side::Reader, 3, 2, ahead.a002[2].id),
            (Some(6), Side::Reader, 4, 3, ahead.a002[3].id),
        ]
    );
    assert_eq!(labelled.diagnostics.len(), 3, "{:?}", labelled.diagnostics);

    // The labels are exactly those of the same run stated at its turns.
    let dir = fixture::dir("tool-use-on-time");
    let on_time = fixture::write(&dir, &rows_with_a002_ahead(0));
    let reference = label(&on_time);
    assert!(
        reference.diagnostics.is_empty(),
        "{:?}",
        reference.diagnostics
    );
    assert_eq!(labelled.world.labels(), reference.world.labels());
}

#[test]
fn a_turn_past_the_sessions_end_whose_tool_result_is_captured_is_kept() {
    let dir = fixture::dir("tool-use-out-of-range");
    let rows = vec![
        fixture::header(),
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(9, "toolu_r1"),
            "p1",
            P1,
        ),
    ];
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let line2 = transmission_at(&labelled, 2).expect("line 2 is kept");
    assert_eq!(line2.reader_exchange, written.a002[1].id);
    assert_eq!(labelled.resolved.dropped, 0);
    assert_eq!(
        labelled
            .diagnostics
            .entries
            .iter()
            .map(|entry| (
                entry.line,
                entry.row,
                entry.side,
                entry.failure.clone(),
                entry.effect
            ))
            .collect::<Vec<_>>(),
        [(
            Some(2),
            Some(RowKind::Transmission),
            Side::Reader,
            JoinFailure::TurnMismatch {
                session: "session-a002".to_owned(),
                turn: 9,
                found_turn: 1,
                exchange: written.a002[1].id,
            },
            Effect::Kept
        )]
    );
}

#[test]
fn a_tool_result_captured_nowhere_is_dropped_as_tool_use_missing() {
    let dir = fixture::dir("tool-use-nowhere");
    let mut gone = miss(9);
    gone["reader_tool_use_id"] = json!("toolu_gone");
    let rows = vec![
        fixture::header(),
        // Within the session's turns.
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(1, "toolu_nowhere"),
            "p1",
            P1,
        ),
        // Past its end.
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(9, "toolu_nowhere"),
            "p1",
            P1,
        ),
        gone,
    ];
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let failures: Vec<_> = labelled
        .diagnostics
        .entries
        .iter()
        .map(|entry| (entry.line, entry.side, entry.failure.clone(), entry.effect))
        .collect();
    let missing = |turn, tool_use_id: &str| JoinFailure::ToolUseMissing {
        session: "session-a002".to_owned(),
        turn,
        tool_use_id: tool_use_id.to_owned(),
    };
    assert_eq!(
        failures,
        [
            (
                Some(2),
                Side::Reader,
                missing(1, "toolu_nowhere"),
                Effect::Dropped
            ),
            (
                Some(3),
                Side::Reader,
                missing(9, "toolu_nowhere"),
                Effect::Dropped
            ),
            (
                Some(4),
                Side::Reader,
                missing(9, "toolu_gone"),
                Effect::Dropped
            ),
        ]
    );
    assert_eq!(labelled.resolved.dropped, 3);
    assert_eq!(labelled.resolved.transmissions, 0);
}

#[test]
fn a_writer_turn_that_is_off_still_finds_the_put_by_tool_use_id() {
    let dir = fixture::dir("tool-use-writer-off");
    let rows = vec![
        fixture::header(),
        // a001's PUT p1 is at turn 0; its turn 1 response is PUT p2.
        fixture::delivery(
            "transmission",
            a001(1, "toolu_w1"),
            a002(1, "toolu_r1"),
            "p1",
            P1,
        ),
        // Past the session's end.
        fixture::delivery(
            "transmission",
            a001(9, "toolu_w2"),
            a003(1, "toolu_r3"),
            "p2",
            P2,
        ),
    ];
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    assert_eq!(
        transmission_at(&labelled, 2).and_then(|label| label.sender_exchange),
        Some(written.a001[0].id)
    );
    assert_eq!(
        transmission_at(&labelled, 3).and_then(|label| label.sender_exchange),
        Some(written.a001[1].id)
    );
    assert_eq!(labelled.resolved.without_sender, 0);
    assert_eq!(
        turn_mismatches(&labelled),
        [
            (Some(2), Side::Writer, 1, 0, written.a001[0].id),
            (Some(3), Side::Writer, 9, 1, written.a001[1].id),
        ]
    );
    assert_eq!(labelled.diagnostics.len(), 2, "{:?}", labelled.diagnostics);
}

#[test]
fn a_put_held_by_two_responses_joins_the_first_and_is_reported() {
    let dir = fixture::dir("tool-use-writer-repeated");
    let rows = vec![
        fixture::header(),
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(1, "toolu_r1"),
            "p1",
            P1,
        ),
    ];
    let written = fixture::write_shaped(
        &dir,
        &rows,
        Shape {
            repeated_put: true,
            ..Shape::default()
        },
    );
    assert_eq!(written.a001.len(), 5);
    let labelled = label(&written);
    assert_eq!(
        transmission_at(&labelled, 2).and_then(|label| label.sender_exchange),
        Some(written.a001[0].id)
    );
    assert_eq!(
        labelled
            .diagnostics
            .entries
            .iter()
            .map(|entry| (entry.line, entry.side, entry.failure.clone(), entry.effect))
            .collect::<Vec<_>>(),
        [(
            Some(2),
            Side::Writer,
            JoinFailure::ToolUseRepeated {
                session: "session-a001".to_owned(),
                tool_use_id: "toolu_w1".to_owned(),
                exchanges: 2,
                exchange: written.a001[0].id,
            },
            Effect::Noted
        )]
    );
}

#[test]
fn a_hash_mismatch_at_the_first_carrying_exchange_still_drops_the_row() {
    let dir = fixture::dir("tool-use-hash");
    let rows = vec![
        fixture::header(),
        // Stated one turn ahead, with another body's hash: the stale turn
        // carries the same result, but the first carrying exchange decides.
        fixture::delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(2, "toolu_r1"),
            "p1",
            "another body",
        ),
    ];
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    assert_eq!(
        labelled
            .diagnostics
            .entries
            .iter()
            .map(|entry| (entry.line, entry.side, entry.failure.clone(), entry.effect))
            .collect::<Vec<_>>(),
        [(
            Some(2),
            Side::Reader,
            JoinFailure::HashMismatch {
                session: "session-a002".to_owned(),
                turn: 2,
                tool_use_id: "toolu_r1".to_owned(),
                exchange: written.a002[1].id,
            },
            Effect::Dropped
        )]
    );
    assert_eq!(labelled.resolved.dropped, 1);
    assert!(transmission_at(&labelled, 2).is_none());
}

#[test]
fn correct_turns_report_no_turn_diagnostic() {
    let dir = fixture::dir("tool-use-correct");
    let written = fixture::write(&dir, &rows_with_a002_ahead(0));
    let labelled = label(&written);
    assert!(
        labelled.diagnostics.is_empty(),
        "{:?}",
        labelled.diagnostics
    );
    assert_eq!(
        transmission_at(&labelled, 2).map(|label| label.reader_exchange),
        Some(written.a002[1].id)
    );
    assert_eq!(
        transmission_at(&labelled, 7).map(|label| label.reader_exchange),
        Some(written.a003[2].id)
    );
}

#[test]
fn the_join_by_tool_use_id_is_dataset_version_2() {
    // Labels changed for runs whose turns ran ahead of the capture.
    assert_eq!(a2a_bench_dataset_demo_swarm::VERSION, 2);
}
