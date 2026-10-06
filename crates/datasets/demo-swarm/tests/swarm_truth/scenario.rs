//! The truth header's `scenario`: which dataset id a run is exported
//! under.

use a2a_bench_dataset_demo_swarm::schema::Scenario;
use a2a_bench_dataset_demo_swarm::truth_file::TruthFileError;
use a2a_bench_dataset_demo_swarm::{BOILERPLATE, HEADLINE};
use serde_json::json;

use super::fixture::{self, Shape};
use super::{label, read_rows};

/// The fixture's truth with the header's `scenario` set (`None`: absent).
fn truth_with(scenario: Option<&str>) -> Vec<serde_json::Value> {
    let mut rows = fixture::truth_rows();
    if let Some(scenario) = scenario {
        rows[0]["scenario"] = json!(scenario);
    }
    rows
}

#[test]
fn the_header_keys_are_pinned_with_an_optional_scenario() {
    let mut pinned = vec![
        "kind",
        "version",
        "world",
        "run",
        "seed",
        "agents",
        "keys",
        "agents_per_key",
        "claude_code_shape",
        "started_at_unix_ms",
        "gateway_url",
        "wiki_url",
    ];
    let keys = |header: &serde_json::Value| {
        let mut keys: Vec<String> = header
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        keys.sort_unstable();
        keys
    };
    pinned.sort_unstable();
    assert_eq!(keys(&fixture::header()), pinned);
    pinned.push("scenario");
    pinned.sort_unstable();
    assert_eq!(keys(&truth_with(Some("headline"))[0]), pinned);
}

#[test]
fn a_header_without_a_scenario_is_the_headline() {
    let truth = read_rows(&[fixture::header()]).expect("decodes");
    assert_eq!(truth.header.scenario, None);
    assert_eq!(truth.header.scenario(), Scenario::Headline);
    assert_eq!(Scenario::Headline.dataset().unwrap().as_str(), HEADLINE);
}

#[test]
fn a_header_names_either_scenario() {
    for (name, scenario) in [
        ("headline", Scenario::Headline),
        ("boilerplate", Scenario::Boilerplate),
    ] {
        let truth = read_rows(&truth_with(Some(name))[..1]).expect("decodes");
        assert_eq!(truth.header.scenario, Some(scenario));
        assert_eq!(
            scenario.dataset().unwrap().as_str(),
            format!("demo-swarm/{name}").as_str()
        );
    }
    assert_eq!(Scenario::Boilerplate.dataset_name(), BOILERPLATE);
}

#[test]
fn an_unknown_scenario_is_refused() {
    for bad in [json!("oracle"), json!(null), json!(1)] {
        let mut header = fixture::header();
        header["scenario"] = bad;
        assert!(matches!(
            read_rows(&[header]),
            Err(TruthFileError::Decode { line: 1, .. })
        ));
    }
}

#[test]
fn a_run_is_labelled_under_its_scenarios_dataset() {
    for (name, scenario, dataset) in [
        ("scenario-absent", None, HEADLINE),
        ("scenario-headline", Some("headline"), HEADLINE),
        ("scenario-boilerplate", Some("boilerplate"), BOILERPLATE),
    ] {
        let dir = fixture::dir(name);
        let written = fixture::write_as(
            &dir,
            &truth_with(scenario),
            Shape::default(),
            dataset,
            fixture::WORLD,
        );
        let labelled = label(&written);
        assert_eq!(labelled.world.dataset().as_str(), dataset);
        assert!(!labelled.world.labels().is_empty());
    }
}

#[test]
fn the_swarms_header_line_decodes_in_its_key_order() {
    // As crates/demo writes it: `scenario` right after `version`.
    let line = r#"{"kind":"header","version":2,"scenario":"boilerplate","world":"swarm-01J0000000000000000000000A","run":"01J0000000000000000000000A","seed":7,"agents":5,"keys":3,"agents_per_key":2,"claude_code_shape":true,"started_at_unix_ms":1000,"gateway_url":"http://crosstalk:8080/anthropic","wiki_url":"http://wiki:8090"}"#;
    let header: serde_json::Value = serde_json::from_str(line).expect("json");
    let truth = read_rows(&[header]).expect("decodes");
    assert_eq!(truth.header.scenario(), Scenario::Boilerplate);
    assert_eq!(truth.header.agents, 5);
}
