//! Label rows: checked on construction and on deserialization.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_format::ids::SourceRef;
use a2a_bench_format::labels::{
    AgentCluster, ClusterFields, ClusterKind, ControlFields, Exemption, ExemptionFields,
    ExemptionReason, ExpectedAccess, ExpectedTransmission, InvalidLabel, Label, MatchClass,
    MatchNeed, NegativeControl, NegativeReason, Route, Tier,
};

#[test]
fn a_valid_transmission_round_trips() {
    let fixture = common::fixture();
    let label = Label::Transmission(common::transmission(&fixture));
    let json = serde_json::to_string(&label).unwrap();
    assert!(
        json.starts_with(r#"{"kind":"transmission","id":"t1""#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Label>(&json).unwrap(), label);
}

#[test]
fn transmission_checks() {
    let fixture = common::fixture();
    let mut fields = common::transmission_fields(&fixture);
    fields.to = common::agent("alice");
    assert!(matches!(
        ExpectedTransmission::new(fields),
        Err(InvalidLabel::SelfTransmission(_))
    ));

    let mut fields = common::transmission_fields(&fixture);
    fields.content.text.push('!');
    assert!(matches!(
        ExpectedTransmission::new(fields),
        Err(InvalidLabel::ContentLength { .. })
    ));

    let mut fields = common::transmission_fields(&fixture);
    fields.reader_exchange = fixture.b1();
    assert!(matches!(
        ExpectedTransmission::new(fields),
        Err(InvalidLabel::ElsewhereThanReader { .. })
    ));

    let mut fields = common::transmission_fields(&fixture);
    fields.needs = MatchNeed::Undecodable {
        codec: "rot13".into(),
    };
    assert_eq!(
        ExpectedTransmission::new(fields.clone()),
        Err(InvalidLabel::Reach)
    );
    fields.tier = Tier::OutOfReach;
    assert!(ExpectedTransmission::new(fields).is_ok());

    let mut fields = common::transmission_fields(&fixture);
    fields.tier = Tier::OutOfReach;
    fields.needs = MatchNeed::Unobserved {
        reason: "sender_medium_unobserved".into(),
        arrival: MatchClass::Exact,
    };
    assert!(ExpectedTransmission::new(fields).is_ok());
}

#[test]
fn deserializing_runs_the_checks() {
    let fixture = common::fixture();
    let mut value =
        serde_json::to_value(Label::Transmission(common::transmission(&fixture))).unwrap();
    value["to"] = serde_json::json!("alice");
    assert!(serde_json::from_value::<Label>(value).is_err());
}

#[test]
fn access_only_needs_a_channel() {
    let fixture = common::fixture();
    let fields = common::transmission_fields(&fixture);
    assert!(ExpectedAccess::new(fields.clone()).is_ok());
    let mut direct = fields;
    direct.route = Route::Direct;
    assert_eq!(
        ExpectedAccess::new(direct),
        Err(InvalidLabel::AccessOffChannel)
    );
}

fn control(fixture: &common::Fixture) -> ControlFields {
    ControlFields {
        id: common::label_id("c1"),
        from: common::agent("alice"),
        to: common::agent("bob"),
        reader_exchange: Some(fixture.b2()),
        at: None,
        origin: None,
        text: None,
        reason: NegativeReason::Reread,
        tier: Tier::Construction,
        source: SourceRef::new("fixture.json", "/controls/0"),
    }
}

#[test]
fn negative_controls_are_bounded_and_self_reads_allowed() {
    let fixture = common::fixture();
    assert!(NegativeControl::new(control(&fixture)).is_ok());
    let mut unbounded = control(&fixture);
    unbounded.reader_exchange = None;
    assert_eq!(
        NegativeControl::new(unbounded),
        Err(InvalidLabel::Unbounded)
    );
    let mut self_reread = control(&fixture);
    self_reread.to = common::agent("alice");
    assert!(matches!(
        NegativeControl::new(self_reread.clone()),
        Err(InvalidLabel::SelfTransmission(_))
    ));
    self_reread.reason = NegativeReason::SelfRead;
    assert!(NegativeControl::new(self_reread).is_ok());
    let mut elsewhere = control(&fixture);
    elsewhere.at = Some(fixture.secret_in_result());
    elsewhere.reader_exchange = Some(fixture.b1());
    assert!(matches!(
        NegativeControl::new(elsewhere),
        Err(InvalidLabel::ElsewhereThanReader { .. })
    ));
}

#[test]
fn exemptions_sit_in_their_reader_exchange() {
    let fixture = common::fixture();
    let fields = ExemptionFields {
        id: common::label_id("e1"),
        to: common::agent("bob"),
        reader_exchange: fixture.b2(),
        at: fixture.secret_in_result(),
        text: None,
        reason: ExemptionReason::UnknownSender,
        tier: Tier::Structural,
        source: SourceRef::new("f", "/e"),
    };
    assert!(Exemption::new(fields.clone()).is_ok());
    let mut elsewhere = fields;
    elsewhere.reader_exchange = fixture.a1();
    assert!(Exemption::new(elsewhere).is_err());
}

#[test]
fn clusters_need_two_distinct_agents() {
    let fields = |agents: &[&str]| ClusterFields {
        id: common::label_id("k1"),
        agents: agents.iter().map(|a| common::agent(a)).collect(),
        cluster: ClusterKind::KeyGroup,
        tier: Tier::Structural,
        source: SourceRef::new("f", "/k"),
    };
    assert!(AgentCluster::new(fields(&["alice", "bob"])).is_ok());
    assert_eq!(
        AgentCluster::new(fields(&["alice"])),
        Err(InvalidLabel::SmallCluster)
    );
    assert_eq!(
        AgentCluster::new(fields(&["alice", "alice"])),
        Err(InvalidLabel::SmallCluster)
    );
}

#[test]
fn tiers_outside_overall() {
    assert!(Tier::Construction.in_overall());
    assert!(!Tier::OutOfReach.in_overall());
    assert!(!Tier::Forwarding.in_overall());
}

#[test]
fn a_cluster_row_round_trips_through_the_label_tag() {
    let cluster = AgentCluster::new(ClusterFields {
        id: common::label_id("k1"),
        agents: vec![common::agent("alice"), common::agent("bob")],
        cluster: ClusterKind::KeyGroup,
        tier: Tier::Structural,
        source: SourceRef::new("f", "/k"),
    })
    .unwrap();
    let label = Label::AgentCluster(cluster);
    let json = serde_json::to_string(&label).unwrap();
    assert_eq!(json.matches("\"kind\"").count(), 1, "{json}");
    assert_eq!(serde_json::from_str::<Label>(&json).unwrap(), label);
}

#[test]
fn every_label_kind_round_trips() {
    let fixture = common::fixture();
    let rows = vec![
        Label::Transmission(common::transmission(&fixture)),
        Label::AccessOnly(ExpectedAccess::new(common::transmission_fields(&fixture)).unwrap()),
        Label::NegativeControl(NegativeControl::new(control(&fixture)).unwrap()),
    ];
    for row in rows {
        let json = serde_json::to_string(&row).unwrap();
        assert_eq!(serde_json::from_str::<Label>(&json).unwrap(), row);
    }
}

#[test]
fn need_helpers_match_crosstalk_eval() {
    use a2a_bench_format::labels::{Codec, json_escapes};
    assert_eq!(
        MatchNeed::through_json_string("plain text"),
        MatchNeed::Exact
    );
    assert_eq!(
        MatchNeed::through_json_string("say \"hi\""),
        MatchNeed::Decoded {
            codecs: vec![Codec::JsonString]
        }
    );
    assert!(json_escapes("a\nb") && json_escapes("back\\slash") && !json_escapes("é"));
    assert_eq!(
        MatchNeed::two_string_levels(),
        MatchNeed::Undecodable {
            codec: "json_string+json_string".into()
        }
    );
    assert_eq!(
        MatchNeed::two_string_levels().tier(Tier::Construction),
        Tier::OutOfReach
    );
    assert_eq!(
        MatchNeed::Exact.tier(Tier::Construction),
        Tier::Construction
    );
    assert!(MatchNeed::sender_medium_unobserved(MatchClass::Exact).out_of_reach());
}
