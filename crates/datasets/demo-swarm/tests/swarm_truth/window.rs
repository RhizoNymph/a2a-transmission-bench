//! The run window: a reused session id's exchanges from an earlier run in
//! the same capture are left out of the world, the session ordinals and
//! the agent map.

use a2a_bench_dataset_demo_swarm::{
    DEFAULT_LEAD_MS, DEFAULT_SLACK_MS, Effect, JoinFailure, Labelled, Margins, Options, RunWindow,
    Side,
};
use a2a_bench_format::labels::NegativeReason;
use a2a_bench_format::time::Timestamp;
use serde_json::json;

use super::fixture::{self, P1, START_MS, Written};
use super::{controls_of, key, label, label_with, read_rows, transmissions};

/// The latest time the fixture's rows name (the miss, at `START_MS` + 101 s).
const LATEST_MS: u64 = START_MS + 101_000;

fn margins(lead_ms: u64, slack_ms: u64) -> Margins {
    Margins { lead_ms, slack_ms }
}

fn labelled(name: &str, prior: bool) -> (Written, Labelled) {
    let dir = fixture::dir(name);
    let written = if prior {
        fixture::write_with_prior_run(&dir, &fixture::truth_rows())
    } else {
        fixture::write(&dir, &fixture::truth_rows())
    };
    let labelled = label(&written);
    (written, labelled)
}

/// The diagnostics table without the run window's own rows.
fn other_diagnostics(
    labelled: &Labelled,
) -> Vec<a2a_bench_dataset_demo_swarm::diagnostics::DiagnosticCount> {
    labelled
        .diagnostics
        .table()
        .into_iter()
        .filter(|count| count.failure != "session_reused_outside_run")
        .collect()
}

// ---- the window ----

#[test]
fn the_window_runs_from_the_header_less_the_lead_to_the_latest_row_plus_the_slack() {
    let truth = read_rows(&fixture::truth_rows()).expect("decodes");
    let found = RunWindow::of(&truth, Margins::default());
    assert_eq!(DEFAULT_SLACK_MS, 60_000);
    assert_eq!(DEFAULT_LEAD_MS, 5_000);
    assert_eq!(
        found,
        RunWindow {
            start_unix_ms: START_MS - 5_000,
            end_unix_ms: LATEST_MS + 60_000,
        }
    );
    assert_eq!(
        RunWindow::of(&truth, margins(7, 5)),
        RunWindow {
            start_unix_ms: START_MS - 7,
            end_unix_ms: LATEST_MS + 5,
        },
        "the lead and the slack are configurable"
    );
}

#[test]
fn a_rows_read_and_write_times_extend_the_window() {
    let mut late = fixture::delivery(
        "transmission",
        ("a001", "session-a001", 0, "toolu_w1"),
        ("a002", "session-a002", 1, "toolu_r1"),
        "p1",
        P1,
    );
    late["at_unix_ms"] = json!(START_MS + 1_000);
    late["read_at_unix_ms"] = json!(START_MS + 9_000);
    late["written_at_unix_ms"] = json!(START_MS + 7_000);
    let truth = read_rows(&[fixture::header(), late.clone()]).expect("decodes");
    assert_eq!(
        RunWindow::of(&truth, margins(0, 0)).end_unix_ms,
        START_MS + 9_000
    );

    late["read_at_unix_ms"] = json!(START_MS + 1_000);
    let truth = read_rows(&[fixture::header(), late]).expect("decodes");
    assert_eq!(
        RunWindow::of(&truth, margins(0, 0)).end_unix_ms,
        START_MS + 7_000
    );
}

#[test]
fn a_truth_with_no_timed_row_ends_at_its_start_plus_the_slack() {
    let cluster = json!({"kind": "agent_cluster", "world": fixture::WORLD, "key_group": 0,
        "agents": ["a001"]});
    let truth = read_rows(&[fixture::header(), cluster]).expect("decodes");
    assert_eq!(
        RunWindow::of(&truth, margins(0, 1_000)),
        RunWindow {
            start_unix_ms: START_MS,
            end_unix_ms: START_MS + 1_000,
        }
    );
}

#[test]
fn both_ends_of_the_window_are_inclusive() {
    let window = RunWindow {
        start_unix_ms: START_MS,
        end_unix_ms: LATEST_MS,
    };
    let at = Timestamp::from_micros;
    assert!(!window.contains(at(START_MS * 1000 - 1)));
    assert!(window.contains(at(START_MS * 1000)));
    assert!(window.contains(at(LATEST_MS * 1000 + 999)));
    assert!(!window.contains(at((LATEST_MS + 1) * 1000)));
}

// ---- a run without reuse ----

#[test]
fn a_run_without_reuse_excludes_nothing() {
    let (written, labelled) = labelled("window-no-reuse", false);
    assert!(written.prior_a002.is_empty());
    assert_eq!(labelled.resolved.excluded_outside_window, 0);
    assert_eq!(labelled.resolved.exchanges, 11);
    assert_eq!(labelled.world.exchanges().len(), 11);
    assert_eq!(labelled.capture.outside_window, 0);
    assert_eq!(
        labelled
            .diagnostics
            .named("session_reused_outside_run")
            .count(),
        0
    );
    assert_eq!(
        labelled.window,
        RunWindow {
            start_unix_ms: START_MS - DEFAULT_LEAD_MS,
            end_unix_ms: LATEST_MS + DEFAULT_SLACK_MS,
        }
    );
}

#[test]
fn a_smaller_slack_leaves_out_the_later_exchanges() {
    let dir = fixture::dir("window-no-slack");
    let written = fixture::write(&dir, &fixture::truth_rows());
    let options = Options {
        run_slack_ms: 0,
        ..Options::default()
    };
    let labelled = label_with(&written, &options);
    // The run's exchanges start at T0 + 10 s, + 20 s, …, + 110 s; the rows
    // end at T0 + 101 s, so only a003's last turn is past it.
    assert_eq!(labelled.window.end_unix_ms, LATEST_MS);
    assert_eq!(labelled.resolved.exchanges, 10);
    assert_eq!(labelled.resolved.excluded_outside_window, 1);
    assert_eq!(labelled.world.exchanges().len(), 10);
    assert!(labelled.world.exchange(written.a003[2].id).is_none());
}

// ---- the lead ----

#[test]
fn an_exchange_just_before_the_headers_start_is_in_the_run() {
    // The run's clock is 12 s behind the header's: its first exchange
    // started 2 s before `started_at_unix_ms`.
    let dir = fixture::dir("window-lead");
    let written = fixture::write_skewed(&dir, &fixture::truth_rows(), 12);
    let labelled = label(&written);
    assert_eq!(labelled.resolved.exchanges, 11);
    assert_eq!(labelled.resolved.excluded_outside_window, 0);
    assert_eq!(
        labelled
            .diagnostics
            .named("session_reused_outside_run")
            .count(),
        0
    );
    assert_eq!(labelled.diagnostics.named("turn_mismatch").count(), 1);

    // Without the lead, the exchange before the start (a001's turn 0, at
    // -2 s) is left out.
    let options = Options {
        run_lead_ms: 0,
        ..Options::default()
    };
    let labelled = label_with(&written, &options);
    assert_eq!(labelled.resolved.excluded_outside_window, 1);
    let excluded: Vec<_> = labelled
        .diagnostics
        .named("session_reused_outside_run")
        .map(|diagnostic| diagnostic.failure.clone())
        .collect();
    let expected: Vec<_> = written.a001[..1]
        .iter()
        .map(|turn| JoinFailure::SessionReusedOutsideRun {
            session: "session-a001".to_owned(),
            exchange: turn.id,
        })
        .collect();
    assert_eq!(excluded, expected);
}

#[test]
fn the_lead_does_not_reach_an_earlier_run_an_hour_before() {
    let truth = read_rows(&fixture::truth_rows()).expect("decodes");
    let window = RunWindow::of(&truth, Margins::default());
    let at = |ms: u64| Timestamp::from_micros(ms * 1000);
    assert!(window.contains(at(START_MS - 2_000)));
    assert!(!window.contains(at(START_MS - 3_600_000)));
    let (_, labelled) = labelled("window-lead-prior", true);
    assert_eq!(labelled.resolved.excluded_outside_window, 2);
    assert_eq!(labelled.resolved.exchanges, 11);
}

// ---- a reused session ----

#[test]
fn a_reused_sessions_earlier_exchanges_are_excluded_and_reported() {
    let (written, labelled) = labelled("window-reuse-excluded", true);
    assert_eq!(written.prior_a002.len(), 2);
    let reused: Vec<_> = labelled
        .diagnostics
        .named("session_reused_outside_run")
        .collect();
    assert_eq!(reused.len(), 2);
    for (diagnostic, turn) in reused.iter().zip(&written.prior_a002) {
        assert_eq!(diagnostic.effect, Effect::Excluded);
        assert_eq!(diagnostic.side, Side::Row);
        assert_eq!(diagnostic.line, None);
        assert_eq!(
            diagnostic.failure,
            JoinFailure::SessionReusedOutsideRun {
                session: "session-a002".to_owned(),
                exchange: turn.id,
            }
        );
        assert!(labelled.world.exchange(turn.id).is_none());
    }
    assert_eq!(labelled.resolved.excluded_outside_window, 2);
    assert_eq!(labelled.capture.outside_window, 2);
    let excluded = labelled
        .diagnostics
        .table()
        .into_iter()
        .find(|count| count.failure == "session_reused_outside_run")
        .expect("a table row");
    assert_eq!((excluded.effect, excluded.count), (Effect::Excluded, 2));
    assert!(
        labelled
            .diagnostics
            .render()
            .contains("| - | row | session_reused_outside_run | excluded | 2 |")
    );
}

#[test]
fn the_world_holds_in_window_exchanges_only() {
    let (_, plain) = labelled("window-traffic-plain", false);
    let (_, reused) = labelled("window-traffic-reused", true);
    assert_eq!(reused.resolved.exchanges, 11);
    assert_eq!(reused.world.exchanges().len(), 11);
    // The fixture's ids differ between the two captures (the earlier run
    // takes the first counters); the exchanges' times and sessions do not.
    let ids = |labelled: &Labelled| {
        labelled
            .world
            .exchanges()
            .iter()
            .map(|exchange| (exchange.at_us, exchange.client.session.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&reused), ids(&plain));
}

#[test]
fn ordinals_count_only_the_in_window_exchanges() {
    let (written, reused) = labelled("window-ordinals", true);
    let label = transmissions(&reused)
        .into_iter()
        .find(|label| label.to == key("a002"))
        .expect("line 2 is labelled");
    // The earlier run's second turn holds the same tool result; the join
    // lands on this run's.
    assert_eq!(label.reader_exchange, written.a002[1].id);
    assert_ne!(label.reader_exchange, written.prior_a002[1].id);
    let rereads = controls_of(&reused, NegativeReason::Reread);
    assert_eq!(rereads[0].reader_exchange, Some(written.a002[2].id));

    // Every other join is as in a run without reuse: one turn mismatch
    // (line 7), no new one.
    let (_, plain) = labelled("window-ordinals-plain", false);
    assert_eq!(other_diagnostics(&reused), other_diagnostics(&plain));
    assert_eq!(reused.diagnostics.named("turn_mismatch").count(), 1);
}
