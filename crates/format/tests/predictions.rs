//! Prediction rows: a transmission's state decides which evidence it carries.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_format::ids::{DetectorAgent, TransmissionRef};
use a2a_bench_format::labels::{CarrierKind, Codec, MatchClass};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::predictions::{
    CoAccess, ContentEvidence, InvalidTransmission, MatchKind, PredictedRoute, Prediction, Quality,
    State, Transmission, TransmissionFields,
};
use a2a_bench_format::resource::Resource;

pub fn d(name: &str) -> DetectorAgent {
    DetectorAgent::new(name).unwrap()
}

pub fn content(fixture: &common::Fixture, kind: MatchKind) -> ContentEvidence {
    ContentEvidence {
        from: d("A"),
        to: d("B"),
        reader_exchange: fixture.b2(),
        read_at: fixture.secret_in_result(),
        origin_at: None,
        kind,
        carrier: CarrierKind::ToolResult,
        route: PredictedRoute::Channel {
            resources: vec![Resource::Url(common::PAGE.into())],
        },
    }
}

pub fn co_access(fixture: &common::Fixture) -> CoAccess {
    let post = common::post_call();
    let text = post.part_text(1).unwrap().len();
    CoAccess {
        from: d("A"),
        to: d("B"),
        write_exchange: fixture.a1(),
        write_at: Location {
            exchange: fixture.a1(),
            message: post.id(),
            part: 1,
            range: ByteRange::new(0, u32::try_from(text).unwrap()).unwrap(),
        },
        reader_exchange: fixture.b2(),
        read_at: fixture.secret_in_result(),
        resource: Resource::Url(common::PAGE.into()),
    }
}

fn fields(
    state: State,
    quality: Option<Quality>,
    matches: Vec<ContentEvidence>,
    co: Vec<CoAccess>,
) -> TransmissionFields {
    TransmissionFields {
        id: TransmissionRef::new("t1").unwrap(),
        state,
        quality,
        matches,
        co_access: co,
    }
}

const EXACT_RESULT: Quality = Quality::Content {
    class: MatchClass::Exact,
    carrier: CarrierKind::ToolResult,
};

#[test]
fn confirmed_needs_matches_and_one_of_their_qualities() {
    let fixture = common::fixture();
    let exact = content(&fixture, MatchKind::Exact);
    assert!(
        Transmission::new(fields(
            State::Confirmed,
            Some(EXACT_RESULT),
            vec![exact.clone()],
            vec![]
        ))
        .is_ok()
    );
    assert!(matches!(
        Transmission::new(fields(State::Confirmed, Some(EXACT_RESULT), vec![], vec![])),
        Err(InvalidTransmission::MissingContent { .. })
    ));
    assert!(matches!(
        Transmission::new(fields(State::Confirmed, None, vec![exact.clone()], vec![])),
        Err(InvalidTransmission::MissingContent { .. })
    ));
    let user_turn = Quality::Content {
        class: MatchClass::Exact,
        carrier: CarrierKind::UserTurn,
    };
    assert_eq!(
        Transmission::new(fields(
            State::Classified,
            Some(user_turn),
            vec![exact],
            vec![]
        )),
        Err(InvalidTransmission::QualityNotAMatch)
    );
}

#[test]
fn decoded_matches_name_codecs() {
    let fixture = common::fixture();
    let decoded = Quality::Content {
        class: MatchClass::Decoded,
        carrier: CarrierKind::ToolResult,
    };
    let none = content(&fixture, MatchKind::Decoded { codecs: vec![] });
    assert_eq!(
        Transmission::new(fields(State::Confirmed, Some(decoded), vec![none], vec![])),
        Err(InvalidTransmission::NoCodec)
    );
    let json = content(
        &fixture,
        MatchKind::Decoded {
            codecs: vec![Codec::JsonString],
        },
    );
    assert!(Transmission::new(fields(State::Confirmed, Some(decoded), vec![json], vec![])).is_ok());
}

#[test]
fn access_states_carry_co_access_only() {
    let fixture = common::fixture();
    let co = co_access(&fixture);
    assert!(
        Transmission::new(fields(
            State::Suspected,
            Some(Quality::Suspected),
            vec![],
            vec![co.clone()]
        ))
        .is_ok()
    );
    assert!(
        Transmission::new(fields(
            State::Discarded,
            Some(Quality::Discarded),
            vec![],
            vec![co.clone()]
        ))
        .is_ok()
    );
    assert!(matches!(
        Transmission::new(fields(
            State::Discarded,
            Some(Quality::Suspected),
            vec![],
            vec![co.clone()]
        )),
        Err(InvalidTransmission::Quality { .. })
    ));
    assert!(matches!(
        Transmission::new(fields(
            State::Suspected,
            Some(Quality::Suspected),
            vec![],
            vec![]
        )),
        Err(InvalidTransmission::MissingAccess { .. })
    ));
    let exact = content(&fixture, MatchKind::Exact);
    assert!(matches!(
        Transmission::new(fields(
            State::Suspected,
            Some(Quality::Suspected),
            vec![exact],
            vec![co]
        )),
        Err(InvalidTransmission::UnexpectedContent { .. })
    ));
}

#[test]
fn undecided_states_carry_nothing() {
    let fixture = common::fixture();
    assert!(Transmission::new(fields(State::Detected, None, vec![], vec![])).is_ok());
    assert!(matches!(
        Transmission::new(fields(
            State::AwaitingContent,
            None,
            vec![],
            vec![co_access(&fixture)]
        )),
        Err(InvalidTransmission::UnexpectedEvidence { .. })
    ));
}

#[test]
fn rows_round_trip_through_their_checks() {
    let fixture = common::fixture();
    let row = Prediction::Transmission(
        Transmission::new(fields(
            State::Confirmed,
            Some(EXACT_RESULT),
            vec![content(&fixture, MatchKind::Exact)],
            vec![],
        ))
        .unwrap(),
    );
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains(r#""match":{"class":"exact"}"#), "{json}");
    assert_eq!(serde_json::from_str::<Prediction>(&json).unwrap(), row);
    let mut value = serde_json::to_value(&row).unwrap();
    value["quality"] = serde_json::json!({"kind": "suspected"});
    assert!(serde_json::from_value::<Prediction>(value).is_err());
}
