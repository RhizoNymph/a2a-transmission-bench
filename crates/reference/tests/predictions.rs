//! How the matcher's hits become prediction rows: attribution by
//! credential, one transmission per (reader exchange, sender, route), and
//! the quality of its strongest match.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_format::ids::ExchangeId;
use a2a_bench_format::labels::{CarrierKind, Codec, MatchClass};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::predictions::{
    Attribution, ContentEvidence, MatchKind, PredictedRoute, Prediction, Quality, State,
};
use a2a_bench_reference::predict::{confirmed, strongest, transmission_ref};
use a2a_bench_reference::{ReferenceConfig, ReferenceError, run};
use common::{
    WorldBuilder, agent_of, detector_agent, found, matched, says, system, transmissions, user,
};

const SENTENCE: &str = "The vendor table has eleven overdue approvals in March";
const OTHER: &str = "A second independent sentence about quarterly risk reviews";

fn attributions(predictions: &[Prediction]) -> Vec<Attribution> {
    predictions
        .iter()
        .filter_map(|p| match p {
            Prediction::Attribution(row) => Some(row.clone()),
            Prediction::Unattributed(_) | Prediction::Transmission(_) => None,
        })
        .collect()
}

#[test]
fn every_exchange_is_attributed_to_its_credential() {
    let mut builder = WorldBuilder::new(&["alice", "bob"]);
    let a1 = builder.exchange("alice", 1, vec![user("go")], vec![says(SENTENCE)]);
    let b1 = builder.exchange("bob", 2, vec![user(SENTENCE)], vec![says("ok")]);
    let a2 = builder.exchange(
        "alice",
        3,
        vec![user("go"), says(SENTENCE), user("more")],
        vec![says("done")],
    );
    let inputs = builder.finish();
    let (output, _) = matched(&inputs);
    assert_eq!(
        attributions(&output.predictions),
        vec![
            Attribution {
                agent: agent_of("alice"),
                exchanges: vec![a1, a2],
            },
            Attribution {
                agent: agent_of("bob"),
                exchanges: vec![b1],
            },
        ]
    );
    // Attribution rows come first, then transmissions.
    let first_transmission = output
        .predictions
        .iter()
        .position(|p| matches!(p, Prediction::Transmission(_)));
    assert_eq!(first_transmission, Some(2));
}

#[test]
fn a_shared_credential_is_one_detector_agent() {
    // The reference has no identity inference: two agents behind one key
    // are one detector agent, and what they pass each other is a self-read.
    let mut builder = WorldBuilder::new(&["alice", "bob"]);
    let a = builder.exchange_with(
        "k:group",
        "alice",
        1,
        vec![user("go")],
        vec![says(SENTENCE)],
    );
    let b = builder.exchange_with("k:group", "bob", 2, vec![user(SENTENCE)], vec![says("ok")]);
    let inputs = builder.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(
        attributions(&output.predictions),
        vec![Attribution {
            agent: detector_agent("k:group"),
            exchanges: vec![a, b],
        }]
    );
    assert!(found.is_empty());
}

#[test]
fn a_credential_that_is_no_agent_name_fails_the_world() {
    let mut builder = WorldBuilder::new(&["alice"]);
    builder.exchange_with("", "alice", 1, vec![user("go")], vec![says(SENTENCE)]);
    let inputs = builder.finish();
    let error = run(&inputs, ReferenceConfig::default()).unwrap_err();
    assert!(
        matches!(error, ReferenceError::Credential { .. }),
        "{error:?}"
    );
}

#[test]
fn a_transmission_is_confirmed_with_its_strongest_match() {
    let mut builder = WorldBuilder::new(&["alice", "bob"]);
    builder.exchange(
        "alice",
        1,
        vec![user("go")],
        vec![says(&format!("{SENTENCE}. {OTHER}."))],
    );
    // The normalized hit comes first in scan order, the exact one second.
    builder.exchange(
        "bob",
        2,
        vec![
            system("You are Bob."),
            user(&OTHER.to_uppercase()),
            user(SENTENCE),
        ],
        vec![says("ok")],
    );
    let inputs = builder.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(transmissions(&output), 1);
    // Matches are stored in location order (the format's rule), which need
    // not be scan order; both hits are there.
    let mut classes: Vec<MatchClass> = found.iter().map(|f| f.evidence.kind.class()).collect();
    classes.sort();
    assert_eq!(classes, vec![MatchClass::Exact, MatchClass::Normalized]);
    let Some(Prediction::Transmission(transmission)) = output.predictions.last() else {
        panic!("a transmission row")
    };
    assert_eq!(transmission.fields().state, State::Confirmed);
    assert_eq!(
        transmission.fields().quality,
        Some(Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::UserTurn,
        })
    );
    assert!(transmission.fields().co_access.is_empty());
}

fn evidence(kind: MatchKind, carrier: CarrierKind, at: u32) -> ContentEvidence {
    let exchange = ExchangeId::from_raw(1);
    let message = common::user("x").id();
    let location = Location {
        exchange,
        message,
        part: 0,
        range: ByteRange::new(at, at + 1).unwrap(),
    };
    ContentEvidence {
        from: detector_agent("a"),
        to: detector_agent("b"),
        reader_exchange: exchange,
        read_at: location,
        origin_at: None,
        kind,
        carrier,
        route: PredictedRoute::Direct,
    }
}

#[test]
fn the_strongest_match_is_the_first_of_the_strongest_class() {
    // Classes order Exact, Normalized, Decoded, Semantic; the carrier is
    // the first match's of that class in stored order (crosstalk-spec's
    // `MatchClass::strongest_match`).
    let decoded = MatchKind::Decoded {
        codecs: vec![Codec::Base64],
    };
    let matches = vec![
        evidence(decoded.clone(), CarrierKind::ToolResult, 0),
        evidence(MatchKind::Normalized, CarrierKind::SystemPrompt, 1),
        evidence(MatchKind::Normalized, CarrierKind::UserTurn, 2),
        evidence(decoded, CarrierKind::UserTurn, 3),
    ];
    assert_eq!(
        strongest(&matches),
        Some(Quality::Content {
            class: MatchClass::Normalized,
            carrier: CarrierKind::SystemPrompt,
        })
    );
    let mut with_exact = matches;
    with_exact.push(evidence(MatchKind::Exact, CarrierKind::ToolResult, 4));
    assert_eq!(
        strongest(&with_exact),
        Some(Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::ToolResult,
        })
    );
    assert_eq!(strongest(&[]), None);
}

#[test]
fn confirmed_matches_are_stored_sorted_by_read_location() {
    // Out of location order, the first normalized match is the system
    // prompt's; in location order (stored), it is the user turn's.
    let matches = vec![
        evidence(MatchKind::Normalized, CarrierKind::SystemPrompt, 2),
        evidence(MatchKind::Normalized, CarrierKind::UserTurn, 0),
        evidence(MatchKind::Semantic, CarrierKind::ToolResult, 1),
    ];
    let id = transmission_ref(ExchangeId::from_raw(1), &detector_agent("a"), "direct").unwrap();
    let transmission = confirmed(id, matches).unwrap();
    let fields = transmission.fields();
    let starts: Vec<u32> = fields
        .matches
        .iter()
        .map(|m| m.read_at.range.start())
        .collect();
    assert_eq!(starts, vec![0, 1, 2]);
    assert_eq!(
        fields.quality,
        Some(Quality::Content {
            class: MatchClass::Normalized,
            carrier: CarrierKind::UserTurn,
        })
    );
}

#[test]
fn transmission_ids_are_stable_and_distinct() {
    let build = || {
        let mut builder = WorldBuilder::new(&["alice", "bob", "carol"]);
        builder.exchange("alice", 1, vec![user("go")], vec![says(SENTENCE)]);
        builder.exchange("carol", 2, vec![user("go")], vec![says(OTHER)]);
        builder.exchange(
            "bob",
            3,
            vec![user(SENTENCE), user(OTHER)],
            vec![says("ok")],
        );
        builder.finish()
    };
    let (a, found_a) = matched(&build());
    let (b, _) = matched(&build());
    assert_eq!(a, b);
    assert_eq!(transmissions(&a), 2);
    let ids: std::collections::BTreeSet<_> = found_a.iter().map(|f| &f.transmission).collect();
    assert_eq!(ids.len(), 2);
    let senders: Vec<_> = found(&a).into_iter().map(|f| f.evidence.from).collect();
    assert_eq!(senders, vec![agent_of("alice"), agent_of("carol")]);
}
