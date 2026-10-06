//! The join: truth rows to the capture's exchanges by session + turn,
//! checked by tool use id and BLAKE3, and the labels it makes.

use a2a_bench_dataset_demo_swarm::{Effect, JoinFailure, RowKind, Side};
use a2a_bench_format::ids::{LabelId, SourceRef};
use a2a_bench_format::labels::{
    AgentCluster, CarrierKind, ClusterKind, Codec, ControlFields, ExemptionReason, InvalidLabel,
    Label, MatchNeed, NegativeControl, NegativeReason, Route, Tier,
};
use a2a_bench_format::resource::Resource;
use serde_json::json;

use super::fixture::{self, P1, P2};
use super::{controls_of, exemptions, key, label, labelled, transmission_at, transmissions};

#[test]
fn a_transmission_joins_to_the_readers_exchange_and_tool_result() {
    let (written, labelled) = labelled("join");
    let label = transmission_at(&labelled, 2).expect("line 2 is labelled");
    assert_eq!(label.id.as_str(), "line/2");
    assert_eq!(label.from, key("a001"));
    assert_eq!(label.to, key("a002"));
    assert_eq!(label.reader_exchange, written.a002[1].id);
    assert_eq!(label.sender_exchange, Some(written.a001[0].id));
    assert_eq!(
        label.route,
        Route::Channel {
            resource: Resource::Url("http://wiki:8090/pages/p1".to_owned())
        }
    );
    assert_eq!(label.carrier, CarrierKind::ToolResult);
    assert_eq!(label.tier, Tier::Construction);
    assert_eq!(label.content.text, P1);
    assert_eq!(label.content.at.exchange, written.a002[1].id);
    assert_eq!(
        label.content.at.message,
        written.a002[1].last_tool.expect("a tool result")
    );
    assert_eq!(label.content.at.part, 0);
    assert_eq!(label.content.at.range.start(), 0);
    assert_eq!(label.content.at.range.end() as usize, P1.len());
    assert_eq!(label.source, SourceRef::new("truth.jsonl", "line/2"));
    // P1 holds a quote and a newline, which the writer's PUT escapes: one
    // JSON string level, not normalization.
    assert_eq!(
        label.needs,
        MatchNeed::Decoded {
            codecs: vec![Codec::JsonString]
        }
    );
    // P2 holds nothing JSON escapes.
    let plain = transmission_at(&labelled, 3).expect("line 3 is labelled");
    assert_eq!(plain.needs, MatchNeed::Exact);
    assert_eq!(plain.content.text, P2);
    assert_eq!(labelled.resolved.transmissions, 3);
    assert_eq!(labelled.resolved.without_sender, 0);
}

#[test]
fn self_reads_rereads_and_misses_become_controls() {
    let (written, labelled) = labelled("controls");
    let self_reads = controls_of(&labelled, NegativeReason::SelfRead);
    assert_eq!(self_reads.len(), 1);
    let self_read = self_reads[0];
    assert_eq!(self_read.from, key("a001"));
    assert_eq!(self_read.to, key("a001"));
    assert_eq!(self_read.reader_exchange, Some(written.a001[3].id));
    assert_eq!(self_read.text.as_deref(), Some(P1));
    let rereads = controls_of(&labelled, NegativeReason::Reread);
    assert_eq!(rereads.len(), 1);
    let reread = rereads[0];
    assert_eq!((&reread.from, &reread.to), (&key("a001"), &key("a002")));
    assert_eq!(reread.reader_exchange, Some(written.a002[2].id));
    let misses = controls_of(&labelled, NegativeReason::Miss);
    let senders: Vec<_> = misses.iter().map(|control| &control.from).collect();
    assert_eq!(senders, [&key("a001"), &key("a003")]);
    let ids: Vec<&str> = misses.iter().map(|control| control.id.as_str()).collect();
    assert_eq!(ids, ["line/6/a001", "line/6/a003"]);
    assert!(misses.iter().all(|control| control.to == key("a002")
        && control.reader_exchange == Some(written.a002[3].id)
        && control.at.is_some()
        && control.text.is_none()));
    assert_eq!(labelled.resolved.self_reads, 1);
    assert_eq!(labelled.resolved.rereads, 1);
    assert_eq!(labelled.resolved.misses, 1);
    assert_eq!(labelled.resolved.miss_controls, 2);
}

#[test]
fn a_turn_mismatch_is_reported_and_joined_by_the_tool_result() {
    let (written, labelled) = labelled("turn-mismatch");
    let mismatches: Vec<_> = labelled.diagnostics.named("turn_mismatch").collect();
    assert_eq!(mismatches.len(), 1);
    let diagnostic = mismatches[0];
    assert_eq!(diagnostic.line, Some(7));
    assert_eq!(diagnostic.row, Some(RowKind::Transmission));
    assert_eq!(diagnostic.side, Side::Reader);
    assert_eq!(diagnostic.effect, Effect::Kept);
    assert_eq!(
        diagnostic.failure,
        JoinFailure::TurnMismatch {
            session: "session-a003".to_owned(),
            turn: 1,
            found_turn: 2,
            exchange: written.a003[2].id,
        }
    );
    let label = transmission_at(&labelled, 7).expect("line 7 is labelled");
    assert_eq!(label.reader_exchange, written.a003[2].id);
}

#[test]
fn a_hash_mismatch_is_reported_and_drops_the_row() {
    let (written, labelled) = labelled("hash-mismatch");
    let mismatches: Vec<_> = labelled.diagnostics.named("hash_mismatch").collect();
    assert_eq!(mismatches.len(), 1);
    let diagnostic = mismatches[0];
    assert_eq!(diagnostic.line, Some(8));
    assert_eq!(diagnostic.side, Side::Reader);
    assert_eq!(diagnostic.effect, Effect::Dropped);
    assert_eq!(
        diagnostic.failure,
        JoinFailure::HashMismatch {
            session: "session-a003".to_owned(),
            turn: 1,
            tool_use_id: "toolu_r3".to_owned(),
            exchange: written.a003[1].id,
        }
    );
    assert_eq!(labelled.resolved.dropped, 1);
    assert!(transmission_at(&labelled, 8).is_none());
    let rendered = labelled.diagnostics.render();
    assert!(rendered.contains("| transmission | reader | hash_mismatch | dropped | 1 |"));
    assert!(rendered.contains("| transmission | reader | turn_mismatch | kept | 1 |"));
}

#[test]
fn a_single_agent_key_group_is_reported_not_labelled() {
    let (_, labelled) = labelled("key-groups");
    assert_eq!(labelled.key_groups.len(), 1);
    assert_eq!(labelled.resolved.key_groups, 1);
    assert_eq!(labelled.resolved.clusters, 0);
    let noted: Vec<_> = labelled
        .diagnostics
        .named("key_group_not_a_cluster")
        .collect();
    assert_eq!(noted.len(), 1);
    assert_eq!(noted[0].effect, Effect::Noted);
    assert_eq!(
        noted[0].failure,
        JoinFailure::KeyGroupNotACluster {
            key_group: 0,
            agents: 1
        }
    );
    assert!(
        !labelled
            .world
            .labels()
            .iter()
            .any(|label| matches!(label, Label::AgentCluster(_)))
    );
}

#[test]
fn a_shared_key_group_is_a_key_group_cluster_not_an_identity() {
    let dir = fixture::dir("key-group-shared");
    let mut rows = fixture::truth_rows();
    rows.push(
        json!({"kind": "agent_cluster", "world": fixture::WORLD, "key_group": 1,
        "agents": ["a003", "a002"]}),
    );
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let clusters: Vec<_> = labelled
        .world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::AgentCluster(cluster) => Some(cluster.fields()),
            _ => None,
        })
        .collect();
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].kind, ClusterKind::KeyGroup);
    assert_eq!(clusters[0].agents, [key("a002"), key("a003")]);
    assert_eq!(clusters[0].id.as_str(), "line/10");
    assert_eq!(labelled.resolved.key_groups, 2);
    assert_eq!(labelled.resolved.clusters, 1);
    assert_eq!(
        labelled
            .diagnostics
            .named("key_group_not_a_cluster")
            .count(),
        1
    );
}

#[test]
fn an_unknown_session_and_a_missing_turn_are_reported() {
    let dir = fixture::dir("missing");
    let rows = vec![
        fixture::header(),
        fixture::delivery(
            "transmission",
            ("a001", "session-a001", 0, "toolu_w1"),
            ("a009", "session-a009", 1, "toolu_r1"),
            "p1",
            P1,
        ),
        fixture::delivery(
            "transmission",
            ("a001", "session-a001", 0, "toolu_w1"),
            ("a002", "session-a002", 9, "toolu_nowhere"),
            "p1",
            P1,
        ),
        fixture::delivery(
            "transmission",
            ("a001", "session-a001", 0, "toolu_nowhere"),
            ("a003", "session-a003", 1, "toolu_r3"),
            "p2",
            P2,
        ),
    ];
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let failures: Vec<(Option<usize>, Side, &str, Effect)> = labelled
        .diagnostics
        .entries
        .iter()
        .map(|entry| (entry.line, entry.side, entry.failure.name(), entry.effect))
        .collect();
    assert_eq!(
        failures,
        [
            (Some(2), Side::Reader, "unknown_session", Effect::Dropped),
            (Some(3), Side::Reader, "turn_out_of_range", Effect::Dropped),
            (
                Some(4),
                Side::Writer,
                "tool_use_missing",
                Effect::KeptWithoutSender
            ),
        ]
    );
    assert_eq!(labelled.resolved.transmissions, 1);
    assert_eq!(labelled.resolved.without_sender, 1);
    assert_eq!(
        transmission_at(&labelled, 4).map(|label| label.sender_exchange),
        Some(None)
    );
}

#[test]
fn a_write_that_is_not_a_put_leaves_the_label_without_a_sender() {
    let dir = fixture::dir("not-a-put");
    // a001's turn 2 response is `GET p1` under toolu_self.
    let rows = vec![
        fixture::header(),
        fixture::delivery(
            "transmission",
            ("a001", "session-a001", 2, "toolu_self"),
            ("a002", "session-a002", 1, "toolu_r1"),
            "p1",
            P1,
        ),
    ];
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let names: Vec<_> = labelled
        .diagnostics
        .entries
        .iter()
        .map(|entry| (entry.side, entry.failure.clone(), entry.effect))
        .collect();
    assert_eq!(
        names,
        [(
            Side::Writer,
            JoinFailure::NotAPut {
                session: "session-a001".to_owned(),
                tool_use_id: "toolu_self".to_owned(),
                exchange: written.a001[2].id,
            },
            Effect::KeptWithoutSender
        )]
    );
    assert_eq!(labelled.resolved.without_sender, 1);
}

#[test]
fn a_url_that_is_no_url_drops_the_row() {
    let dir = fixture::dir("bad-url");
    let mut row = fixture::delivery(
        "transmission",
        ("a001", "session-a001", 0, "toolu_w1"),
        ("a002", "session-a002", 1, "toolu_r1"),
        "p1",
        P1,
    );
    row["route"]["url"] = json!("not a url");
    let written = fixture::write(&dir, &[fixture::header(), row]);
    let labelled = label(&written);
    let bad: Vec<_> = labelled.diagnostics.named("bad_url").collect();
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].effect, Effect::Dropped);
    assert_eq!(labelled.resolved.dropped, 1);
    assert!(transmissions(&labelled).is_empty());
}

#[test]
fn the_route_is_the_normalized_url() {
    let dir = fixture::dir("route-normalized");
    let mut row = fixture::delivery(
        "transmission",
        ("a001", "session-a001", 0, "toolu_w1"),
        ("a002", "session-a002", 1, "toolu_r1"),
        "p1",
        P1,
    );
    row["route"]["url"] = json!("HTTP://Wiki:8090/pages/p1#top");
    let written = fixture::write(&dir, &[fixture::header(), row]);
    let labelled = label(&written);
    let label = transmission_at(&labelled, 2).expect("line 2 is labelled");
    assert_eq!(
        label.route,
        Route::Channel {
            resource: Resource::Url("http://wiki:8090/pages/p1".to_owned())
        }
    );
}

// ---- unattributed reads ----

/// The fixture's self-read and reread rows and, about a002's first read of
/// p1, at most that it was unattributed.
fn unattributed_truth(with_row: bool) -> Vec<serde_json::Value> {
    let all = fixture::truth_rows();
    let mut rows = vec![fixture::header(), all[3].clone(), all[4].clone()];
    if with_row {
        rows.push(fixture::unattributed(
            ("a002", "session-a002", 1, "toolu_r1"),
            "p1",
            P1,
        ));
    }
    rows
}

#[test]
fn an_unattributed_read_becomes_an_exemption_at_the_read() {
    let dir = fixture::dir("unattributed-row");
    let written = fixture::write(&dir, &unattributed_truth(true));
    let labelled = label(&written);
    assert_eq!(labelled.resolved.unattributed, 1);
    assert!(
        labelled.diagnostics.is_empty(),
        "{:?}",
        labelled.diagnostics
    );
    let exempt = exemptions(&labelled);
    assert_eq!(exempt.len(), 1);
    let exemption = exempt[0];
    assert_eq!(exemption.to, key("a002"));
    assert_eq!(exemption.reader_exchange, written.a002[1].id);
    assert_eq!(exemption.at.exchange, written.a002[1].id);
    assert_eq!(exemption.reason, ExemptionReason::UnknownSender);
    assert_eq!(exemption.tier, Tier::Construction);
    assert_eq!(exemption.text.as_deref(), Some(P1));
    assert_eq!(exemption.id.as_str(), "line/4");
}

#[test]
fn without_the_row_there_is_no_exemption() {
    let dir = fixture::dir("unattributed-none");
    let written = fixture::write(&dir, &unattributed_truth(false));
    let labelled = label(&written);
    assert_eq!(labelled.resolved.unattributed, 0);
    assert!(exemptions(&labelled).is_empty());
}

#[test]
fn an_unattributed_read_with_the_wrong_hash_is_dropped() {
    let dir = fixture::dir("unattributed-hash");
    let mut rows = unattributed_truth(false);
    rows.push(fixture::unattributed(
        ("a002", "session-a002", 1, "toolu_r1"),
        "p1",
        "another body",
    ));
    let written = fixture::write(&dir, &rows);
    let labelled = label(&written);
    let mismatches: Vec<_> = labelled.diagnostics.named("hash_mismatch").collect();
    assert_eq!(mismatches.len(), 1);
    assert_eq!(mismatches[0].row, Some(RowKind::UnattributedRead));
    assert_eq!(mismatches[0].effect, Effect::Dropped);
    assert_eq!(labelled.resolved.dropped, 1);
    assert!(exemptions(&labelled).is_empty());
}

// ---- the format's constructors, as ct-eval's truth types behaved ----

#[test]
fn only_a_self_read_control_names_one_agent_twice() {
    let fields = |reason| ControlFields {
        id: LabelId::new("line/1").unwrap(),
        from: key("a001"),
        to: key("a001"),
        reader_exchange: None,
        at: None,
        origin: None,
        text: Some("x".to_owned()),
        reason,
        tier: Tier::Construction,
        source: SourceRef::new("truth.jsonl", "line/1"),
    };
    // Bounded by nothing: refused for being unbounded, not for the pair.
    assert_eq!(
        NegativeControl::new(fields(NegativeReason::SelfRead)),
        Err(InvalidLabel::Unbounded)
    );
    assert_eq!(
        NegativeControl::new(fields(NegativeReason::Reread)),
        Err(InvalidLabel::SelfTransmission(key("a001")))
    );
    assert_eq!(
        NegativeControl::new(fields(NegativeReason::Miss)),
        Err(InvalidLabel::SelfTransmission(key("a001")))
    );
    let one = a2a_bench_format::labels::ClusterFields {
        id: LabelId::new("line/1").unwrap(),
        agents: vec![key("a001")],
        kind: ClusterKind::KeyGroup,
        tier: Tier::Construction,
        source: SourceRef::new("truth.jsonl", "line/1"),
    };
    assert_eq!(AgentCluster::new(one), Err(InvalidLabel::SmallCluster));
}
