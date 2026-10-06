//! The CLI scores with the bench's resource canonicaliser: a channel
//! label's resource and a predicted resource that differ only in canonical
//! form align, where the scorer's default (`AsGiven`) does not.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use a2a_bench_cli::canon::ResourceCanon;
use a2a_bench_format::ids::{
    AgentKey, DatasetId, LabelId, SourceRef, TransmissionRef, exchange_id,
};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedContent, ExpectedTransmission, MatchClass, MatchNeed, Route, Tier,
    TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{Body, Message, UserPart};
use a2a_bench_format::predictions::{PredictedRoute, Quality};
use a2a_bench_format::resource::Resource;
use a2a_bench_format::time::Timestamp;
use a2a_bench_score::AsGiven;
use a2a_bench_score::canon::same_resource;
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::Prediction;
use a2a_bench_score::score::align::aligns;

const TEXT: &str = "the vendor table moved";
const LABELLED: &str = "https://example.com/wiki/page?a=1&b=2";
const PREDICTED: &str = "HTTPS://Example.COM:443/wiki/page?b=2&a=1#section";

fn pair() -> (ExpectedTransmission, Prediction) {
    let message = Message::new(Body::User(vec![UserPart::Text { text: TEXT.into() }])).unwrap();
    let dataset = DatasetId::new("synthetic").unwrap();
    let reader = exchange_id(
        &dataset,
        &SourceRef::new("f", "/0"),
        Timestamp::from_micros(1_767_225_600_000_000),
    );
    let at = Location {
        exchange: reader,
        message: message.id(),
        part: 0,
        range: ByteRange::new(0, u32::try_from(TEXT.len()).unwrap()).unwrap(),
    };
    let (alice, bob) = (
        AgentKey::new("alice").unwrap(),
        AgentKey::new("bob").unwrap(),
    );
    let label = ExpectedTransmission::new(TransmissionFields {
        id: LabelId::new("l1").unwrap(),
        from: alice.clone(),
        to: bob.clone(),
        sender_exchange: None,
        reader_exchange: reader,
        route: Route::Channel {
            resource: Resource::Url(LABELLED.into()),
        },
        carrier: CarrierKind::ToolResult,
        content: ExpectedContent {
            text: TEXT.into(),
            at,
        },
        needs: MatchNeed::Exact,
        tier: Tier::Construction,
        source: SourceRef::new("f", "/labels/l1"),
    })
    .unwrap();
    let prediction = Prediction {
        transmission: TransmissionRef::new("t:1").unwrap(),
        from: alice,
        to: bob,
        reader_exchange: reader,
        route: PredictedRoute::Channel {
            resources: vec![Resource::Url(PREDICTED.into())],
        },
        carrier: CarrierKind::ToolResult,
        class: EvidenceClass::Exact,
        quality: Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::ToolResult,
        },
        read_at: at,
        origin_at: None,
    };
    (label, prediction)
}

#[test]
fn resources_differing_only_in_canonical_form_are_the_same_resource() {
    let (a, b) = (
        Resource::Url(LABELLED.into()),
        Resource::Url(PREDICTED.into()),
    );
    assert!(same_resource(&ResourceCanon, &a, &b));
    assert!(!same_resource(&AsGiven, &a, &b));
}

#[test]
fn a_channel_label_aligns_with_a_prediction_naming_its_resource_in_another_form() {
    let (label, prediction) = pair();
    assert!(aligns(&prediction, &label, &ResourceCanon));
    assert!(!aligns(&prediction, &label, &AsGiven));
}

#[test]
fn a_different_resource_still_does_not_align() {
    let (label, mut prediction) = pair();
    prediction.route = PredictedRoute::Channel {
        resources: vec![Resource::Url("https://example.com/wiki/other".into())],
    };
    assert!(!aligns(&prediction, &label, &ResourceCanon));
}
