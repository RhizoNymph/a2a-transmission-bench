//! A detector's prediction rows as the scorer's predictions: attribution
//! (splits are fine, merges fail the world), unattributed agents, and one
//! prediction per content match or co-access.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use a2a_bench_format::ids::{AgentKey, DetectorAgent, ExchangeId, TransmissionRef};
use a2a_bench_format::labels::{CarrierKind, MatchClass, RouteKind};
use a2a_bench_format::location::Location;
use a2a_bench_format::predictions::{
    Attribution, CoAccess, ContentEvidence, MatchKind, PredictedRoute, Prediction as Row, Quality,
    State, Transmission, TransmissionFields, Unattributed,
};
use a2a_bench_format::resource::Resource;
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::{
    AgentMap, AgentMapError, UnknownDetectedAgent, exchange_agents, from_transmission,
    world_predictions,
};
use common::{Built, Draft, calls, complete, result, says, user, whole};

const PAGE: &str = "https://wiki.example/plan";
const SECRET: &str = "the meeting moved to 4pm";

struct Scene {
    built: Built,
    alice: AgentKey,
    bob: AgentKey,
    /// Alice's post.
    a1: ExchangeId,
    /// Bob's request for the page.
    b1: ExchangeId,
    /// Bob's read of it.
    b2: ExchangeId,
    write_at: Location,
    read_at: Location,
}

fn scene() -> Scene {
    let mut draft = Draft::new("w");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    let post = calls(
        "c1",
        "http_request",
        &format!(r#"{{"url":"{PAGE}","method":"POST","body":"{SECRET}"}}"#),
    );
    let get = calls(
        "c2",
        "http_request",
        &format!(r#"{{"url":"{PAGE}","method":"GET"}}"#),
    );
    let page = result("c2", &format!("Plan: {SECRET}."));
    let task = user("Read the plan.");
    let a1 = draft.exchange(&alice, 1, &[&user("Post the plan.")], &[&post]);
    let b1 = draft.exchange(&bob, 2, &[&task], &[&get]);
    let b2 = draft.exchange(&bob, 3, &[&task, &get, &page], &[&says("Noted.")]);
    Scene {
        write_at: whole(a1, &post, 0),
        read_at: whole(b2, &page, 0),
        built: draft.finish(complete()),
        alice,
        bob,
        a1,
        b1,
        b2,
    }
}

fn d(name: &str) -> DetectorAgent {
    DetectorAgent::new(name).unwrap()
}

fn attribution(agent: &str, exchanges: &[ExchangeId]) -> Row {
    Row::Attribution(Attribution {
        agent: d(agent),
        exchanges: exchanges.to_vec(),
    })
}

fn content(scene: &Scene, from: &str, kind: MatchKind) -> ContentEvidence {
    ContentEvidence {
        from: d(from),
        to: d("d:bob"),
        reader_exchange: scene.b2,
        read_at: scene.read_at,
        origin_at: Some(scene.write_at),
        kind,
        carrier: CarrierKind::ToolResult,
        route: PredictedRoute::Channel {
            resources: vec![Resource::Url(PAGE.into())],
        },
    }
}

fn confirmed(scene: &Scene, id: &str, from: &str) -> Row {
    Row::Transmission(
        Transmission::new(TransmissionFields {
            id: TransmissionRef::new(id).unwrap(),
            state: State::Confirmed,
            quality: Some(Quality::Content {
                class: MatchClass::Exact,
                carrier: CarrierKind::ToolResult,
            }),
            matches: vec![
                content(scene, from, MatchKind::Exact),
                content(
                    scene,
                    from,
                    MatchKind::Decoded {
                        codecs: vec![a2a_bench_format::labels::Codec::JsonString],
                    },
                ),
            ],
            co_access: Vec::new(),
        })
        .unwrap(),
    )
}

fn access(scene: &Scene, id: &str, state: State) -> Transmission {
    Transmission::new(TransmissionFields {
        id: TransmissionRef::new(id).unwrap(),
        state,
        quality: Some(if state == State::Suspected {
            Quality::Suspected
        } else {
            Quality::Discarded
        }),
        matches: Vec::new(),
        co_access: vec![CoAccess {
            from: d("d:alice"),
            to: d("d:bob"),
            write_exchange: scene.a1,
            write_at: scene.write_at,
            reader_exchange: scene.b2,
            read_at: scene.read_at,
            resource: Resource::Url(PAGE.into()),
        }],
    })
    .unwrap()
}

fn true_agents(scene: &Scene) -> Vec<Row> {
    vec![
        attribution("d:alice", &[scene.a1]),
        attribution("d:bob", &[scene.b1, scene.b2]),
    ]
}

fn map(scene: &Scene, rows: &[Row]) -> Result<AgentMap, AgentMapError> {
    AgentMap::from_rows(&exchange_agents(&scene.built.labels), rows)
}

#[test]
fn each_detector_agent_is_the_true_agent_of_its_exchanges() {
    let scene = scene();
    let agents = map(&scene, &true_agents(&scene)).unwrap();
    assert_eq!(agents.get(&d("d:alice")), Some(&scene.alice));
    assert_eq!(agents.get(&d("d:bob")), Some(&scene.bob));
    assert_eq!(agents.get(&d("d:nobody")), None);
}

#[test]
fn a_split_is_fine() {
    let scene = scene();
    let rows = vec![
        attribution("d:alice", &[scene.a1]),
        attribution("d:bob-1", &[scene.b1]),
        attribution("d:bob-2", &[scene.b2]),
    ];
    let agents = map(&scene, &rows).unwrap();
    assert_eq!(agents.get(&d("d:bob-1")), Some(&scene.bob));
    assert_eq!(agents.get(&d("d:bob-2")), Some(&scene.bob));
}

#[test]
fn a_merge_fails_the_world_naming_the_ids() {
    let scene = scene();
    let rows = vec![attribution("d:one", &[scene.a1, scene.b1, scene.b2])];
    assert_eq!(
        map(&scene, &rows),
        Err(AgentMapError::Merged {
            agent: d("d:one"),
            first: scene.alice.clone(),
            second: scene.bob.clone(),
        })
    );
    assert!(world_predictions(&scene.built.labels, &rows).is_err());
}

#[test]
fn content_states_make_one_prediction_per_match() {
    let scene = scene();
    let rows = true_agents(&scene);
    let agents = map(&scene, &rows).unwrap();
    let Row::Transmission(transmission) = confirmed(&scene, "t:1", "d:alice") else {
        panic!("a transmission");
    };
    let predictions = from_transmission(&transmission, &agents).unwrap();
    assert_eq!(predictions.len(), 2);
    let classes: Vec<EvidenceClass> = predictions.iter().map(|p| p.class).collect();
    assert_eq!(classes, [EvidenceClass::Exact, EvidenceClass::Decoded]);
    for p in &predictions {
        assert_eq!((&p.from, &p.to), (&scene.alice, &scene.bob));
        assert_eq!(p.reader_exchange, scene.b2);
        assert_eq!(p.read_at, scene.read_at);
        assert_eq!(p.origin_at, Some(scene.write_at));
        assert_eq!(p.carrier, CarrierKind::ToolResult);
        assert_eq!(p.route.kind(), RouteKind::Channel);
        assert_eq!(
            p.quality,
            Quality::Content {
                class: MatchClass::Exact,
                carrier: CarrierKind::ToolResult
            }
        );
        assert_eq!(p.transmission.as_str(), "t:1");
    }
}

#[test]
fn access_states_make_one_prediction_per_co_access() {
    let scene = scene();
    let agents = map(&scene, &true_agents(&scene)).unwrap();
    for (state, class, quality) in [
        (
            State::Suspected,
            EvidenceClass::Suspected,
            Quality::Suspected,
        ),
        (
            State::Discarded,
            EvidenceClass::Discarded,
            Quality::Discarded,
        ),
    ] {
        let predictions = from_transmission(&access(&scene, "t:2", state), &agents).unwrap();
        assert_eq!(predictions.len(), 1);
        let p = &predictions[0];
        assert_eq!((p.class, p.quality), (class, quality));
        assert_eq!((&p.from, &p.to), (&scene.alice, &scene.bob));
        assert_eq!(p.carrier, CarrierKind::ToolResult);
        assert_eq!(
            p.route,
            PredictedRoute::Channel {
                resources: vec![Resource::Url(PAGE.into())]
            }
        );
        assert_eq!(p.reader_exchange, scene.b2);
        assert_eq!(p.read_at, scene.read_at);
        assert_eq!(p.origin_at, Some(scene.write_at));
    }
}

#[test]
fn undecided_states_make_no_prediction() {
    let scene = scene();
    let agents = map(&scene, &true_agents(&scene)).unwrap();
    for state in [State::Detected, State::AwaitingContent] {
        let transmission = Transmission::new(TransmissionFields {
            id: TransmissionRef::new("t:3").unwrap(),
            state,
            quality: None,
            matches: Vec::new(),
            co_access: Vec::new(),
        })
        .unwrap();
        assert!(
            from_transmission(&transmission, &agents)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn an_unattributed_agents_transmissions_are_reported_and_never_scored() {
    let scene = scene();
    let rows = vec![
        attribution("d:bob", &[scene.b1, scene.b2]),
        Row::Unattributed(Unattributed {
            agent: d("d:ghost"),
        }),
        confirmed(&scene, "t:ghost", "d:ghost"),
        Row::Transmission(access(&scene, "t:known", State::Suspected)),
    ];
    // `t:known` names d:alice, who has no row here: unknown as well.
    let made = world_predictions(&scene.built.labels, &rows).unwrap();
    assert!(made.predictions.is_empty());
    assert_eq!(
        made.unknown,
        vec![
            UnknownDetectedAgent {
                transmission: TransmissionRef::new("t:ghost").unwrap(),
                agent: d("d:ghost"),
            },
            UnknownDetectedAgent {
                transmission: TransmissionRef::new("t:known").unwrap(),
                agent: d("d:alice"),
            },
        ]
    );

    let mut attributed = true_agents(&scene);
    attributed.push(Row::Unattributed(Unattributed {
        agent: d("d:ghost"),
    }));
    attributed.push(confirmed(&scene, "t:ghost", "d:ghost"));
    attributed.push(confirmed(&scene, "t:alice", "d:alice"));
    let made = world_predictions(&scene.built.labels, &attributed).unwrap();
    assert_eq!(made.predictions.len(), 2, "t:alice's two matches");
    assert_eq!(made.unknown.len(), 1);
}
