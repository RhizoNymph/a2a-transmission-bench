//! Ids: exchange ids are crosstalk-eval's, bit for bit; keys and digests
//! round-trip and refuse bad text.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use a2a_bench_format::ids::{
    AgentKey, DatasetId, Digest, ExchangeId, InvalidKey, InvalidUlid, SourceRef, WorldKey,
    exchange_id,
};
use a2a_bench_format::time::Timestamp;
use proptest::prelude::*;
use serde_json::Value;

#[test]
fn exchange_ids_match_crosstalk_eval() {
    let vectors: Value =
        serde_json::from_str(include_str!("fixtures/crosstalk-7f8a2fb-vectors.json")).unwrap();
    let cases = vectors["exchange_ids"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let dataset = DatasetId::new(case["dataset"].as_str().unwrap()).unwrap();
        let source = SourceRef::new(
            case["file"].as_str().unwrap(),
            case["path"].as_str().unwrap(),
        );
        let at = Timestamp::from_micros(case["at_us"].as_u64().unwrap());
        let id = exchange_id(&dataset, &source, at);
        assert_eq!(id.to_ulid(), case["id"].as_str().unwrap(), "{case}");
    }
}

#[test]
fn exchange_ids_sort_by_time() {
    let dataset = DatasetId::new("d").unwrap();
    let early = exchange_id(
        &dataset,
        &SourceRef::new("f", "/z"),
        Timestamp::from_micros(1_000),
    );
    let late = exchange_id(
        &dataset,
        &SourceRef::new("f", "/a"),
        Timestamp::from_micros(5_000_000),
    );
    assert!(early < late);
    assert!(early.to_ulid() < late.to_ulid());
}

#[test]
fn ulid_text_is_strict() {
    let id: ExchangeId = "01KDVDNAZ8S5H61TAWJTKTJVEZ".parse().unwrap();
    assert_eq!(id.to_ulid(), "01KDVDNAZ8S5H61TAWJTKTJVEZ");
    assert_eq!(
        "01kdvdnaz8s5h61tawjtktjvez".parse::<ExchangeId>(),
        Err(InvalidUlid::Character { index: 2 })
    );
    assert_eq!(
        "0".parse::<ExchangeId>(),
        Err(InvalidUlid::Length { got: 1 })
    );
    assert_eq!(
        "81KDVDNAZ8S5H61TAWJTKTJVEZ".parse::<ExchangeId>(),
        Err(InvalidUlid::Overflow)
    );
    assert!("01KDVDNAZ8S5H61TAWJTKTJVEI".parse::<ExchangeId>().is_err());
}

#[test]
fn exchange_ids_serialize_as_ulid_text() {
    let id: ExchangeId = "01KDVDNAZ8S5H61TAWJTKTJVEZ".parse().unwrap();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"01KDVDNAZ8S5H61TAWJTKTJVEZ\"");
    assert_eq!(serde_json::from_str::<ExchangeId>(&json).unwrap(), id);
}

#[test]
fn keys_refuse_empty_and_control_text() {
    assert_eq!(
        AgentKey::new(""),
        Err(InvalidKey::Empty { kind: "agent key" })
    );
    assert!(matches!(
        WorldKey::new("a\nb"),
        Err(InvalidKey::Control { at: 1, .. })
    ));
    assert!(WorldKey::new("trace 7 / ü").is_ok());
    assert!(DatasetId::new("demo-swarm/headline").is_ok());
    assert!(matches!(
        DatasetId::new("SALT"),
        Err(InvalidKey::Charset { .. })
    ));
    assert!(serde_json::from_str::<AgentKey>("\"\"").is_err());
}

#[test]
fn digests_are_lower_case_hex() {
    let digest = Digest::keyed("ctx", b"bytes");
    let text = digest.to_hex();
    assert_eq!(text.len(), 64);
    assert_eq!(text.parse::<Digest>().unwrap(), digest);
    assert!(text.to_uppercase().parse::<Digest>().is_err());
    assert!(text[..63].parse::<Digest>().is_err());
}

proptest! {
    #[test]
    fn ulids_round_trip(raw in any::<u128>()) {
        let id = ExchangeId::from_raw(raw);
        prop_assert_eq!(id.to_ulid().parse::<ExchangeId>().unwrap(), id);
    }
}
