//! Checks across a world's files.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_format::check::{
    InputError, LabelError, LocationError, PredictionError, WorldInputs, check_labels,
    check_predictions,
};
use a2a_bench_format::ids::{DetectorAgent, TransmissionRef};
use a2a_bench_format::labels::{CarrierKind, ExchangeAgent, Label, MatchClass};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::predictions::{
    Attribution, CoAccess, ContentEvidence, MatchKind, PredictedRoute, Prediction, Quality, State,
    Transmission, TransmissionFields, Unattributed,
};
use a2a_bench_format::resource::Resource;

fn inputs(fixture: &common::Fixture) -> Result<WorldInputs, InputError> {
    WorldInputs::new(
        &common::world_key(),
        fixture.messages.clone(),
        fixture.decl.clone(),
        fixture.exchanges.clone(),
    )
}

#[test]
fn the_fixture_is_valid() {
    let fixture = common::fixture();
    let world = inputs(&fixture).unwrap();
    check_labels(&world, &fixture.labels()).unwrap();
    assert_eq!(
        world.text_at(&fixture.secret_in_result()).unwrap(),
        common::SECRET
    );
}

#[test]
fn input_errors() {
    let mut fixture = common::fixture();
    fixture.messages.pop();
    assert!(matches!(
        inputs(&fixture),
        Err(InputError::MissingMessage { .. })
    ));

    let mut fixture = common::fixture();
    fixture.messages.push(common::user("never used"));
    assert!(matches!(
        inputs(&fixture),
        Err(InputError::UnusedMessage(_))
    ));

    let mut fixture = common::fixture();
    fixture.messages.push(common::system());
    assert!(matches!(
        inputs(&fixture),
        Err(InputError::DuplicateMessage(_))
    ));

    let mut fixture = common::fixture();
    fixture.exchanges.swap(0, 2);
    assert!(matches!(
        inputs(&fixture),
        Err(InputError::OutOfOrder { .. })
    ));

    let fixture = common::fixture();
    let other = a2a_bench_format::ids::WorldKey::new("w2").unwrap();
    assert!(matches!(
        WorldInputs::new(
            &other,
            fixture.messages.clone(),
            fixture.decl.clone(),
            fixture.exchanges.clone()
        ),
        Err(InputError::WorldMismatch { .. })
    ));
}

#[test]
fn locations_must_resolve() {
    let fixture = common::fixture();
    let world = inputs(&fixture).unwrap();
    let good = fixture.secret_in_result();
    let past = Location {
        range: ByteRange::new(0, 10_000).unwrap(),
        ..good
    };
    assert!(matches!(
        world.text_at(&past),
        Err(LocationError::PastEnd { .. })
    ));
    let elsewhere = Location {
        exchange: fixture.a1(),
        ..good
    };
    assert!(matches!(
        world.text_at(&elsewhere),
        Err(LocationError::MessageNotInExchange { .. })
    ));
    let part = Location { part: 3, ..good };
    assert!(matches!(
        world.text_at(&part),
        Err(LocationError::NoText { .. })
    ));
}

#[test]
fn locations_respect_character_boundaries() {
    let mut fixture = common::fixture();
    let accented = common::user("héllo");
    fixture.messages.push(accented.clone());
    fixture.exchanges[0].request.messages.push(accented.id());
    let world = inputs(&fixture).unwrap();
    let at = |start, end| Location {
        exchange: fixture.a1(),
        message: accented.id(),
        part: 0,
        range: ByteRange::new(start, end).unwrap(),
    };
    assert_eq!(world.text_at(&at(1, 3)).unwrap(), "é");
    assert!(matches!(
        world.text_at(&at(1, 2)),
        Err(LocationError::SplitsCharacter { .. })
    ));
}

#[test]
fn label_errors() {
    let fixture = common::fixture();
    let world = inputs(&fixture).unwrap();
    let mut labels = fixture.labels();
    labels.remove(0);
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::Unassigned(_))
    ));

    let mut labels = fixture.labels();
    labels.push(Label::ExchangeAgent(ExchangeAgent {
        exchange: fixture.a1(),
        agent: common::agent("bob"),
    }));
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::AssignedTwice(_))
    ));

    let mut labels = fixture.labels();
    labels[0] = Label::ExchangeAgent(ExchangeAgent {
        exchange: fixture.a1(),
        agent: common::agent("carol"),
    });
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::Scripted { .. })
    ));

    let mut labels = fixture.labels();
    labels[2] = Label::ExchangeAgent(ExchangeAgent {
        exchange: fixture.b2(),
        agent: common::agent("alice"),
    });
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::WrongAgent { .. })
    ));

    let mut fields = common::transmission_fields(&fixture);
    fields.content.text = "the meeting moved to 5pm".into();
    let mut labels = fixture.labels();
    labels[3] =
        Label::Transmission(a2a_bench_format::labels::ExpectedTransmission::new(fields).unwrap());
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::ContentMismatch { .. })
    ));

    let mut fields = common::transmission_fields(&fixture);
    fields.from = common::agent("dave");
    let mut labels = fixture.labels();
    labels[3] =
        Label::Transmission(a2a_bench_format::labels::ExpectedTransmission::new(fields).unwrap());
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::UnknownAgent { .. })
    ));

    let mut labels = fixture.labels();
    labels.push(labels[3].clone());
    assert!(matches!(
        check_labels(&world, &labels),
        Err(LabelError::DuplicateId(_))
    ));
}

fn d(name: &str) -> DetectorAgent {
    DetectorAgent::new(name).unwrap()
}

fn predictions(fixture: &common::Fixture, read_at: Location) -> Vec<Prediction> {
    let evidence = ContentEvidence {
        from: d("A"),
        to: d("B"),
        reader_exchange: fixture.b2(),
        read_at,
        origin_at: None,
        kind: MatchKind::Exact,
        carrier: CarrierKind::ToolResult,
        route: PredictedRoute::Channel {
            resources: vec![Resource::Url(common::PAGE.into())],
        },
    };
    let transmission = Transmission::new(TransmissionFields {
        id: TransmissionRef::new("t1").unwrap(),
        state: State::Confirmed,
        quality: Some(Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::ToolResult,
        }),
        matches: vec![evidence],
        co_access: vec![],
    })
    .unwrap();
    vec![
        Prediction::Attribution(Attribution {
            agent: d("A"),
            exchanges: vec![fixture.a1()],
        }),
        Prediction::Attribution(Attribution {
            agent: d("B"),
            exchanges: vec![fixture.b1(), fixture.b2()],
        }),
        Prediction::Transmission(transmission),
    ]
}

#[test]
fn prediction_checks() {
    let fixture = common::fixture();
    let world = inputs(&fixture).unwrap();
    check_predictions(&world, &predictions(&fixture, fixture.secret_in_result())).unwrap();

    let mut rows = predictions(&fixture, fixture.secret_in_result());
    rows.remove(0);
    assert!(matches!(
        check_predictions(&world, &rows),
        Err(PredictionError::UnknownAgent { .. })
    ));
    rows.insert(0, Prediction::Unattributed(Unattributed { agent: d("A") }));
    check_predictions(&world, &rows).unwrap();

    let mut rows = predictions(&fixture, fixture.secret_in_result());
    rows[0] = Prediction::Attribution(Attribution {
        agent: d("A"),
        exchanges: vec![fixture.a1(), fixture.b1()],
    });
    assert!(matches!(
        check_predictions(&world, &rows),
        Err(PredictionError::ExchangeTwice { .. })
    ));

    let elsewhere = Location {
        exchange: fixture.b1(),
        ..fixture.secret_in_result()
    };
    let rows = predictions(&fixture, elsewhere);
    assert!(matches!(
        check_predictions(&world, &rows),
        Err(PredictionError::ExchangeMismatch { .. })
    ));

    let mut rows = predictions(&fixture, fixture.secret_in_result());
    rows.push(rows[2].clone());
    assert!(matches!(
        check_predictions(&world, &rows),
        Err(PredictionError::DuplicateTransmission(_))
    ));
}

/// The secret's location split in two: its first byte, then the rest.
fn halves(fixture: &common::Fixture) -> (Location, Location) {
    let whole = fixture.secret_in_result();
    let (start, end) = (whole.range.start(), whole.range.end());
    let first = Location {
        range: ByteRange::new(start, start + 1).unwrap(),
        ..whole
    };
    let rest = Location {
        range: ByteRange::new(start + 1, end).unwrap(),
        ..whole
    };
    (first, rest)
}

/// The fixture's predictions with the transmission's evidence replaced.
fn with_evidence(
    fixture: &common::Fixture,
    matches: Vec<Location>,
    co_access: Vec<(Location, Location)>,
) -> Vec<Prediction> {
    let mut rows = predictions(fixture, fixture.secret_in_result());
    let content = !matches.is_empty();
    let matches = matches
        .into_iter()
        .map(|read_at| ContentEvidence {
            from: d("A"),
            to: d("B"),
            reader_exchange: fixture.b2(),
            read_at,
            origin_at: None,
            kind: MatchKind::Exact,
            carrier: CarrierKind::ToolResult,
            route: PredictedRoute::Direct,
        })
        .collect();
    let co_access = co_access
        .into_iter()
        .map(|(read_at, write_at)| CoAccess {
            from: d("A"),
            to: d("B"),
            write_exchange: fixture.b2(),
            write_at,
            reader_exchange: fixture.b2(),
            read_at,
            resource: Resource::Url(common::PAGE.into()),
        })
        .collect();
    rows[2] = Prediction::Transmission(
        Transmission::new(TransmissionFields {
            id: TransmissionRef::new("t1").unwrap(),
            state: if content {
                State::Confirmed
            } else {
                State::Suspected
            },
            quality: Some(if content {
                Quality::Content {
                    class: MatchClass::Exact,
                    carrier: CarrierKind::ToolResult,
                }
            } else {
                Quality::Suspected
            }),
            matches,
            co_access,
        })
        .unwrap(),
    );
    rows
}

#[test]
fn matches_must_be_sorted_by_read_location() {
    let fixture = common::fixture();
    let world = inputs(&fixture).unwrap();
    let (first, rest) = halves(&fixture);
    check_predictions(&world, &with_evidence(&fixture, vec![first, rest], vec![])).unwrap();
    // Equal locations are sorted.
    check_predictions(&world, &with_evidence(&fixture, vec![first, first], vec![])).unwrap();
    assert!(matches!(
        check_predictions(&world, &with_evidence(&fixture, vec![rest, first], vec![])),
        Err(PredictionError::Unsorted { transmission }) if transmission.as_str() == "t1"
    ));
}

#[test]
fn co_access_must_be_sorted_by_read_then_write_location() {
    let fixture = common::fixture();
    let world = inputs(&fixture).unwrap();
    let (first, rest) = halves(&fixture);
    check_predictions(
        &world,
        &with_evidence(&fixture, vec![], vec![(first, rest), (rest, first)]),
    )
    .unwrap();
    // Same read location: ordered by the write location.
    check_predictions(
        &world,
        &with_evidence(&fixture, vec![], vec![(first, first), (first, rest)]),
    )
    .unwrap();
    assert!(matches!(
        check_predictions(
            &world,
            &with_evidence(&fixture, vec![], vec![(rest, first), (first, rest)])
        ),
        Err(PredictionError::Unsorted { .. })
    ));
    assert!(matches!(
        check_predictions(
            &world,
            &with_evidence(&fixture, vec![], vec![(first, rest), (first, first)])
        ),
        Err(PredictionError::Unsorted { .. })
    ));
}
