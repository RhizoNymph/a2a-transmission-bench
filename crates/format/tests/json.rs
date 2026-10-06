//! Canonical JSON agrees with crosstalk-spec's, by vectors crosstalk
//! computed at 7f8a2fb (`fixtures/crosstalk-7f8a2fb-vectors.json`).
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use a2a_bench_format::json::{CanonicalJson, CanonicalJsonError, Json};
use proptest::prelude::*;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/crosstalk-7f8a2fb-vectors.json")).unwrap()
}

#[test]
fn canonical_text_matches_crosstalk() {
    let vectors = vectors();
    let cases = vectors["canonical_json"].as_array().unwrap();
    assert!(cases.len() >= 19);
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let expected = case["canonical"].as_str().unwrap();
        let got = CanonicalJson::canonicalize(input).unwrap();
        assert_eq!(got.as_str(), expected, "input {input:?}");
    }
}

#[test]
fn rejects_what_crosstalk_rejects() {
    let vectors = vectors();
    for case in vectors["rejects"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        assert!(case["rejected"].as_bool().unwrap());
        assert!(Json::parse(input).is_err(), "accepted {input:?}");
    }
}

#[test]
fn rejects_deep_nesting() {
    let deep = "[".repeat(300) + &"]".repeat(300);
    assert!(Json::parse(&deep).is_err());
    let fine = "[".repeat(200) + &"]".repeat(200);
    assert!(Json::parse(&fine).is_ok());
}

#[test]
fn from_canonical_refuses_other_spellings() {
    assert!(CanonicalJson::from_canonical(r#"{"a":1}"#.into()).is_ok());
    assert_eq!(
        CanonicalJson::from_canonical(r#"{ "a": 1 }"#.into()),
        Err(CanonicalJsonError::NotCanonical)
    );
    assert_eq!(
        CanonicalJson::from_canonical("1.0".into()),
        Err(CanonicalJsonError::NotCanonical)
    );
    assert!(matches!(
        CanonicalJson::from_canonical("[1,".into()),
        Err(CanonicalJsonError::Json(_))
    ));
}

#[test]
fn deserializing_checks_canonical_form() {
    let ok: CanonicalJson = serde_json::from_str(r#""{\"a\":[1,2]}""#).unwrap();
    assert_eq!(ok.as_str(), r#"{"a":[1,2]}"#);
    assert!(serde_json::from_str::<CanonicalJson>(r#""{\"b\":1,\"a\":2}""#).is_err());
}

proptest! {
    #[test]
    fn canonicalizing_is_idempotent(value in arb_json()) {
        let once = CanonicalJson::canonicalize(&value).unwrap();
        let twice = CanonicalJson::canonicalize(once.as_str()).unwrap();
        prop_assert_eq!(once, twice);
    }
}

fn arb_json() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        Just("null".to_owned()),
        any::<bool>().prop_map(|b| b.to_string()),
        any::<i64>().prop_map(|n| n.to_string()),
        (any::<i32>(), 0u32..6).prop_map(|(n, e)| format!("{n}.5e-{e}")),
        "[a-z\\u{e9}\\u{1F600} \"\\\\\n]{0,8}".prop_map(|s| serde_json::to_string(&s).unwrap()),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5)
                .prop_map(|items| format!("[{}]", items.join(","))),
            prop::collection::vec(("[a-zA-Z\\u{1F600}\\u{FB33}]{0,4}", inner), 0..5).prop_map(
                |members| {
                    let body: Vec<String> = members
                        .into_iter()
                        .map(|(k, v)| format!("{}:{}", serde_json::to_string(&k).unwrap(), v))
                        .collect();
                    format!("{{{}}}", body.join(","))
                }
            ),
        ]
    })
}
