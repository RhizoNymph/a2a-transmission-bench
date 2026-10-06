//! The virtual clock's pace: calls of a dataset without times are a
//! deterministic 1 to 5 s apart, in component order; and its outputs, the
//! synthetic credentials and vendors equal what crosstalk computed.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use a2a_bench_corpus::clock::{
    ClockError, EPOCH_MICROS, MIN_STEP, MINOR_LIMIT, Pace, SUB_LIMIT, compose, ordinal,
};
use a2a_bench_corpus::world::{credential_digest, vendor_of};
use a2a_bench_format::ids::{AgentKey, DatasetId, WorldKey};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::time::Timestamp;
use common::ok;
use serde_json::Value;

fn at(pace: &Pace, major: u64, minor: u64, sub: u64) -> u64 {
    ok(pace.at(major, minor, sub)).as_micros()
}

#[test]
fn the_default_pace_steps_one_to_five_seconds() {
    let pace = Pace::DEFAULT;
    assert_eq!(pace.min(), Duration::from_secs(1));
    assert_eq!(pace.max(), Duration::from_secs(5));
    assert_eq!(at(&pace, 0, 0, 0), EPOCH_MICROS);
    let mut gaps = std::collections::BTreeSet::new();
    for major in 0..10_000 {
        let gap = at(&pace, major + 1, 0, 0) - at(&pace, major, 0, 0);
        assert!(
            (1_000_000..=5_000_000).contains(&gap),
            "step {major}: {gap} µs"
        );
        gaps.insert(gap);
    }
    // Jittered, not one fixed step.
    assert!(gaps.len() > 100);
}

#[test]
fn components_keep_their_order_under_any_pace() {
    let slow = ok(Pace::new(Duration::from_secs(1), Duration::from_secs(1), 3));
    for pace in [Pace::DEFAULT, slow] {
        for major in 0..1_000 {
            assert!(at(&pace, major, MINOR_LIMIT - 1, SUB_LIMIT - 1) < at(&pace, major + 1, 0, 0));
            assert!(at(&pace, major, 3, SUB_LIMIT - 1) < at(&pace, major, 4, 0));
        }
    }
}

#[test]
fn a_pace_is_deterministic_and_seeded() {
    let one = |seed| {
        ok(Pace::new(
            Duration::from_secs(1),
            Duration::from_secs(5),
            seed,
        ))
    };
    assert_eq!(one(0), Pace::DEFAULT);
    let times = |pace: Pace| (0..50).map(|m| at(&pace, m, 0, 0)).collect::<Vec<_>>();
    assert_eq!(times(one(9)), times(one(9)));
    assert_ne!(times(one(9)), times(one(10)));
    // `compose` is the default pace.
    for major in 0..50 {
        assert_eq!(
            compose(major, 2, 3).map(|t| t.as_micros()),
            Ok(at(&Pace::DEFAULT, major, 2, 3))
        );
    }
}

#[test]
fn a_pace_steps_within_its_bounds() {
    let pace = ok(Pace::new(
        Duration::from_millis(2_000),
        Duration::from_millis(2_400),
        1,
    ));
    for major in 0..1_000 {
        let gap = at(&pace, major + 1, 0, 0) - at(&pace, major, 0, 0);
        assert!((2_000_000..=2_400_000).contains(&gap), "{gap}");
    }
}

#[test]
fn a_pace_too_fine_for_its_components_is_refused() {
    assert_eq!(
        Pace::new(Duration::from_millis(999), Duration::from_secs(5), 0),
        Err(ClockError::Pace)
    );
    assert_eq!(
        Pace::new(Duration::from_secs(5), Duration::from_secs(1), 0),
        Err(ClockError::Pace)
    );
    assert!(Pace::DEFAULT.at(u64::MAX, 0, 0).is_err());
    assert!(Pace::DEFAULT.at(0, MINOR_LIMIT, 0).is_err());
    assert!(Pace::DEFAULT.at(0, 0, SUB_LIMIT).is_err());
}

#[test]
fn clock_orders_components_and_bounds_them() {
    let at = |major, minor, sub| ok(compose(major, minor, sub));
    assert!(at(0, MINOR_LIMIT - 1, SUB_LIMIT - 1) < at(1, 0, 0));
    assert!(at(3, 5, 999) < at(3, 6, 0));
    assert_eq!(at(0, 0, 0).as_micros(), EPOCH_MICROS);
    assert!(compose(0, MINOR_LIMIT, 0).is_err());
    assert!(compose(0, 0, SUB_LIMIT).is_err());
    assert!(compose(u64::MAX, 0, 0).is_err());
    assert!(ok(ordinal(1)) < ok(ordinal(2)));
}

#[test]
fn a_pace_records_its_settings() {
    let settings = Pace::DEFAULT.settings();
    assert_eq!(settings.get("seed"), Some(&Setting::Int(0)));
    assert_eq!(settings.get("min_ms"), Some(&Setting::Int(1_000)));
    assert_eq!(settings.get("max_ms"), Some(&Setting::Int(5_000)));
    let big = ok(Pace::new(
        Duration::from_secs(1),
        Duration::from_secs(5),
        u64::MAX,
    ));
    assert_eq!(
        big.settings().get("seed"),
        Some(&Setting::Text(u64::MAX.to_string()))
    );
}

// Parity with crosstalk: vectors its own code computed at 7f8a2fb
// (fixtures/crosstalk-corpus-vectors-generator.rs.txt).

fn vectors() -> Value {
    let text = include_str!("fixtures/crosstalk-7f8a2fb-corpus-vectors.json");
    ok(serde_json::from_str(text))
}

fn u64_of(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("not a u64: {value}"))
}

/// A crosstalk result: the time, or `None` for an error.
fn expected_time(value: &Value) -> Option<u64> {
    if value.get("error").is_some() {
        None
    } else {
        Some(u64_of(value))
    }
}

fn got(result: Result<Timestamp, ClockError>) -> Option<u64> {
    result.ok().map(Timestamp::as_micros)
}

#[test]
fn constants_equal_crosstalks() {
    let v = vectors();
    assert_eq!(v["crosstalk"], "7f8a2fb");
    assert_eq!(u64_of(&v["epoch_micros"]), EPOCH_MICROS);
    assert_eq!(u128::from(u64_of(&v["min_step_us"])), MIN_STEP.as_micros());
}

#[test]
fn pace_at_equals_crosstalks() {
    let v = vectors();
    let paces = v["paces"].as_array().cloned().unwrap_or_default();
    assert!(paces.len() >= 9);
    let mut checked = 0;
    for case in &paces {
        let (min, max, seed) = (
            u64_of(&case["min_ms"]),
            u64_of(&case["max_ms"]),
            u64_of(&case["seed"]),
        );
        let pace = Pace::new(Duration::from_millis(min), Duration::from_millis(max), seed);
        let Ok(pace) = pace else {
            assert!(case.get("error").is_some(), "{min}..{max} seed {seed}");
            continue;
        };
        assert!(case.get("error").is_none(), "{min}..{max} seed {seed}");
        assert_eq!(pace.min().as_micros(), u128::from(u64_of(&case["min_us"])));
        assert_eq!(pace.max().as_micros(), u128::from(u64_of(&case["max_us"])));
        for point in case["at"].as_array().cloned().unwrap_or_default() {
            let (major, minor, sub) = (
                u64_of(&point["major"]),
                u64_of(&point["minor"]),
                u64_of(&point["sub"]),
            );
            assert_eq!(
                got(pace.at(major, minor, sub)),
                expected_time(&point["at"]),
                "pace {min}..{max} seed {seed} at ({major}, {minor}, {sub})"
            );
            checked += 1;
        }
    }
    assert!(checked > 1_000, "{checked}");
}

#[test]
fn compose_and_ordinal_equal_crosstalks() {
    let v = vectors();
    for point in v["compose"].as_array().cloned().unwrap_or_default() {
        let (major, minor, sub) = (
            u64_of(&point["major"]),
            u64_of(&point["minor"]),
            u64_of(&point["sub"]),
        );
        assert_eq!(
            got(compose(major, minor, sub)),
            expected_time(&point["at"]),
            "compose({major}, {minor}, {sub})"
        );
    }
    for point in v["ordinal"].as_array().cloned().unwrap_or_default() {
        let n = u64_of(&point["ordinal"]);
        assert_eq!(got(ordinal(n)), expected_time(&point["at"]), "ordinal({n})");
    }
}

#[test]
fn credentials_equal_crosstalks() {
    let v = vectors();
    let cases = v["credentials"].as_array().cloned().unwrap_or_default();
    assert!(!cases.is_empty());
    for case in cases {
        let text = |field: &str| case[field].as_str().unwrap_or_default().to_owned();
        // The format refuses empty keys; crosstalk's digest of them is
        // still pinned, through the raw fields.
        let (Ok(dataset), Ok(world), Ok(agent)) = (
            DatasetId::new(text("dataset")),
            WorldKey::new(text("world")),
            AgentKey::new(text("agent")),
        ) else {
            continue;
        };
        assert_eq!(
            credential_digest(&dataset, &world, &agent).to_hex(),
            text("credential_digest"),
            "{dataset}/{world}/{agent}"
        );
    }
}

#[test]
fn vendors_equal_crosstalks() {
    let v = vectors();
    for case in v["vendors"].as_array().cloned().unwrap_or_default() {
        let model = case["model"].as_str().unwrap_or_default();
        let debug = case["vendor"].as_str().unwrap_or_default();
        let expected = match debug {
            "Anthropic" => "anthropic".to_owned(),
            "Google" => "google".to_owned(),
            "OpenAi" => "openai".to_owned(),
            other => other
                .strip_prefix("Other(\"")
                .and_then(|rest| rest.strip_suffix("\")"))
                .unwrap_or_else(|| panic!("unexpected vendor {other}"))
                .to_owned(),
        };
        assert_eq!(vendor_of(model), expected, "{model}");
    }
}
