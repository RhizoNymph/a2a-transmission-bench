//! The τ²-bench converter on synthetic fixtures shaped like the dataset,
//! ported from crosstalk-eval's `tests/tau2.rs` onto bench types.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{ManifestInfo, export};
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::split::Selection as Split;
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_tau2::files::discover;
use a2a_bench_dataset_tau2::prompts::{
    AGENT_INSTRUCTION, agent_system_prompt, scenario_text, user_system_prompt,
};
use a2a_bench_dataset_tau2::time::parse_time;
use a2a_bench_dataset_tau2::{
    AGENT, DATASET, Options, USER, VERSION, convert_simulation, load_results, source,
};
use a2a_bench_format::exchange::{Driven, Exchange, Fidelity};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, Digest, WorldKey};
use a2a_bench_format::labels::{
    CarrierKind, ControlFields, Label, MatchNeed, NegativeReason, Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::Location;
use a2a_bench_format::manifest::{Converter, Setting, Source};
use a2a_bench_format::message::{Body, Message};
use a2a_bench_format::time::Timestamp;

const FILE: &str = "fixture-agent_airline_default_fixture-user_1trials.json";
const SOLO: &str = "fixture-agent_telecom_no-user_fixture-user_1trials.json";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn world(file: &str, index: usize) -> World {
    let results = load_results(&root(), Path::new(file)).unwrap_or_else(|e| panic!("{e}"));
    convert_simulation(&results, file, index).unwrap_or_else(|e| panic!("{e}"))
}

fn positives(world: &World) -> Vec<&TransmissionFields> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(t) => Some(t.fields()),
            _ => None,
        })
        .collect()
}

fn controls(world: &World, reason: NegativeReason) -> Vec<&ControlFields> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::NegativeControl(c) if c.fields().reason == reason => Some(c.fields()),
            _ => None,
        })
        .collect()
}

/// The exchange whose response is message `index` of simulation `sim`.
fn exchange_of(world: &World, sim: usize, index: usize) -> &Exchange {
    let path = format!("/simulations/{sim}/messages/{index}");
    world
        .exchanges()
        .iter()
        .find(|e| e.source.path() == path)
        .unwrap_or_else(|| panic!("no exchange for message {index}"))
}

fn exchanges_of<'a>(world: &'a World, name: &str) -> Vec<&'a Exchange> {
    world
        .exchanges()
        .iter()
        .filter(|e| world.agent_of(e.id).is_some_and(|a| a.as_str() == name))
        .collect()
}

fn request<'a>(world: &'a World, exchange: &Exchange) -> Vec<&'a Message> {
    exchange
        .request
        .messages
        .iter()
        .map(|id| {
            world
                .message(*id)
                .unwrap_or_else(|| panic!("no message {id}"))
        })
        .collect()
}

fn text_at(world: &World, at: &Location) -> String {
    let exchange = world
        .exchange(at.exchange)
        .unwrap_or_else(|| panic!("no exchange"));
    assert!(
        exchange.side_of(at.message).is_some(),
        "the exchange does not carry the message"
    );
    let message = world
        .message(at.message)
        .unwrap_or_else(|| panic!("no message"));
    let text = message.part_text(at.part).unwrap_or_else(|e| panic!("{e}"));
    text[at.range.start() as usize..at.range.end() as usize].to_owned()
}

/// `YYYY-MM-DDTHH:MM:SS.ffffffZ` on 2025-06-04, as microseconds.
fn time(hour: u64, minute: u64, second: u64, micros: u64) -> Timestamp {
    // 2025-06-04 is day 20,243 after the epoch.
    let seconds = 20_243 * 86_400 + hour * 3600 + minute * 60 + second;
    Timestamp::from_micros(seconds * 1_000_000 + micros)
}

// --- times -----------------------------------------------------------------

#[test]
fn iso_times_parse_with_any_fraction() {
    let parse = |text| parse_time(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(
        parse("2025-06-04T12:22:38.915138"),
        time(12, 22, 38, 915_138)
    );
    assert_eq!(parse("2025-06-04T12:00:10"), time(12, 0, 10, 0));
    assert_eq!(parse("2025-06-04T12:00:10.5"), time(12, 0, 10, 500_000));
    assert_eq!(
        parse("2025-06-04T12:00:10.123456789+00:00"),
        time(12, 0, 10, 123_456)
    );
    assert_eq!(parse("2025-06-04T12:00:10Z"), time(12, 0, 10, 0));
    assert!(parse_time("yesterday").is_err());
    assert!(parse_time("2025-06-04T12:00:10+02:00").is_err());
}

#[test]
fn times_are_checked_as_rfc_3339() {
    assert_eq!(
        parse_time("1970-01-01T00:00:00").unwrap(),
        Timestamp::from_micros(0)
    );
    assert_eq!(
        parse_time("2024-02-29T00:00:00").unwrap(),
        Timestamp::from_micros(19_782 * 86_400 * 1_000_000)
    );
    for bad in [
        "2025-02-29T00:00:00",
        "2025-13-01T00:00:00",
        "2025-04-31T00:00:00",
        "2025-06-04T24:00:00",
        "2025-06-04T12:60:00",
        "2025-06-04T12:00:60",
        "1969-12-31T23:59:59",
        "2025-06-04t12:00:00",
        "2025-06-04T12:00:00.12a",
        "2025-06-04 12:00:00",
    ] {
        assert!(parse_time(bad).is_err(), "{bad}");
    }
}

// --- prompts ---------------------------------------------------------------

#[test]
fn the_agent_prompt_is_the_instruction_and_the_policy() {
    let results = load_results(&root(), Path::new(FILE)).unwrap_or_else(|e| panic!("{e}"));
    let (prompt, fidelity) = agent_system_prompt(&results.info, &results.tasks[0]);
    assert_eq!(
        prompt,
        format!(
            "<instructions>\n{AGENT_INSTRUCTION}\n</instructions>\n<policy>\n# Airline Policy\nBe helpful within the policy.\n\n</policy>"
        )
    );
    assert_eq!(fidelity, Fidelity::Reconstructed);
}

#[test]
fn the_user_prompt_is_the_guidelines_and_the_scenario() {
    let results = load_results(&root(), Path::new(FILE)).unwrap_or_else(|e| panic!("{e}"));
    let scenario = results.tasks[0]
        .user_scenario
        .as_ref()
        .unwrap_or_else(|| panic!("no scenario"));
    let expected = "Persona:\n\tPolite and brief.\nInstructions:\n\tDomain: airline\n\tReason for call:\n\t\tYou want to cancel reservation FIX123.\n\n\t\tYou were out of town.\n\tKnown info:\n\t\tYou are Test Person.\n\t\tYour user id is test_person_0001.\n\tTask instructions:\n\t\tInsist on a refund.";
    assert_eq!(scenario_text(scenario), expected);
    let prompt = user_system_prompt(&results.info, &results.tasks[0]);
    assert_eq!(
        prompt,
        format!(
            "# User Simulation Guidelines\nYou are playing a customer contacting support.\n\nGenerate one message at a time.\n\n\n<scenario>\n{expected}\n</scenario>"
        )
    );
    // Plain-text instructions are used as they are.
    let plain = user_system_prompt(&results.info, &results.tasks[1]);
    assert!(plain.ends_with(
        "<scenario>\nInstructions:\n\tAsk about baggage allowance for reservation FIX456.\n</scenario>"
    ));
}

// --- one conversation ------------------------------------------------------

#[test]
fn agent_and_user_simulator_are_two_model_agents() {
    let world = world(FILE, 0);
    assert_eq!(
        world.key().as_str(),
        "fixture-agent_airline_default_fixture-user_1trials/0"
    );
    let agents = &world.decl().agents;
    assert_eq!(agents.len(), 2);
    let agent = agents
        .iter()
        .find(|a| a.key.as_str() == AGENT)
        .unwrap_or_else(|| panic!("no agent"));
    assert_eq!(agent.driven, Driven::Model);
    assert_eq!(agent.model.as_deref(), Some("fixture-agent"));
    let user = agents
        .iter()
        .find(|a| a.key.as_str() == USER)
        .unwrap_or_else(|| panic!("no user"));
    assert_eq!(user.model.as_deref(), Some("fixture-user"));
    assert_eq!(
        world.coverage(),
        Coverage::Complete {
            tier: Tier::Structural
        }
    );
}

#[test]
fn only_model_calls_are_exchanges_at_their_recorded_times() {
    let world = world(FILE, 0);
    let agent = exchanges_of(&world, AGENT);
    let user = exchanges_of(&world, USER);
    // The greeting (message 0) is hard-coded: no exchange.
    assert_eq!(agent.len(), 4);
    assert_eq!(user.len(), 5);
    assert_eq!(exchange_of(&world, 0, 6).at_us, time(12, 0, 6, 0));
    for exchange in agent.iter().chain(&user) {
        assert_eq!(exchange.fidelity, Fidelity::Reconstructed);
        assert!(exchange.request.tools.is_none());
    }
    // Finish reasons become stops.
    assert_eq!(
        exchange_of(&world, 0, 4).response.stop.as_deref(),
        Some("tool_use")
    );
}

#[test]
fn exchange_ids_derive_from_the_dataset_source_and_time() {
    let world = world(FILE, 0);
    let dataset = DatasetId::new(DATASET).unwrap();
    for exchange in world.exchanges() {
        assert_eq!(
            exchange.id,
            a2a_bench_format::ids::exchange_id(&dataset, &exchange.source, exchange.at_us)
        );
        assert_eq!(exchange.source.file(), FILE);
    }
}

#[test]
fn the_agent_sees_its_own_tools_and_the_users_text() {
    let world = world(FILE, 0);
    let request = request(&world, exchange_of(&world, 0, 10));
    // system, greeting, user 1, agent 2, user 3, call 4, result 5, agent 6,
    // user 9: the user's own tool call (7) and its result (8) are hidden.
    assert_eq!(request.len(), 9);
    assert!(matches!(request[0].body(), Body::System(_)));
    assert!(matches!(request[1].body(), Body::Assistant(_)));
    assert!(matches!(request[2].body(), Body::User(_)));
    assert!(matches!(request[6].body(), Body::Tool(_)));
    assert!(matches!(request[8].body(), Body::User(_)));
}

#[test]
fn the_user_simulator_sees_the_conversation_flipped() {
    let world = world(FILE, 0);
    let request = request(&world, exchange_of(&world, 0, 9));
    // system, greeting, user 1, agent 2, user 3, agent 6, user call 7,
    // result 8: the agent's tool call (4) and its result (5) are hidden.
    assert_eq!(request.len(), 8);
    assert!(matches!(request[0].body(), Body::System(_)));
    assert!(matches!(request[1].body(), Body::User(_)));
    assert!(matches!(request[2].body(), Body::Assistant(_)));
    assert!(matches!(request[5].body(), Body::User(_)));
    assert!(matches!(request[6].body(), Body::Assistant(_)));
    assert!(matches!(request[7].body(), Body::Tool(_)));
    let system = request[0].part_text(0).unwrap_or_default();
    assert!(system.contains("Your user id is test_person_0001."));
}

#[test]
fn each_text_turn_is_a_structural_label_to_the_peers_next_call() {
    let world = world(FILE, 0);
    let labels = positives(&world);
    assert_eq!(labels.len(), 6);
    let expect = |from: &str, sent: usize, read: usize| {
        let reader = exchange_of(&world, 0, read).id;
        let label = labels
            .iter()
            .find(|l| l.reader_exchange == reader && l.from.as_str() == from)
            .unwrap_or_else(|| panic!("no label {from} {sent} -> {read}"));
        assert_eq!(label.sender_exchange, Some(exchange_of(&world, 0, sent).id));
        assert_eq!(label.route, Route::Direct);
        assert_eq!(label.carrier, CarrierKind::UserTurn);
        assert_eq!(label.needs, MatchNeed::Exact);
        assert_eq!(label.tier, Tier::Structural);
        assert_eq!(label.content.at.exchange, reader);
        assert_eq!(text_at(&world, &label.content.at), label.content.text);
    };
    expect(AGENT, 2, 3);
    expect(AGENT, 6, 7);
    expect(AGENT, 10, 12);
    expect(USER, 1, 2);
    expect(USER, 3, 4);
    expect(USER, 9, 10);
}

#[test]
fn the_greeting_is_boilerplate_and_tool_results_are_shared_sources() {
    let world = world(FILE, 0);
    let greeting = controls(&world, NegativeReason::Boilerplate);
    assert_eq!(greeting.len(), 1);
    assert_eq!(greeting[0].from.as_str(), AGENT);
    assert_eq!(greeting[0].to.as_str(), USER);
    assert_eq!(
        greeting[0].reader_exchange,
        Some(exchange_of(&world, 0, 1).id)
    );
    assert_eq!(
        greeting[0].text.as_deref(),
        Some("Hi! How can I help you today?")
    );
    // ct-eval writes three: results 5 and 8, read by the next call, and
    // result 11, read after the agent's last call. No exchange carries
    // result 11, so a bench location cannot place its control, and it is
    // left out.
    let shared = controls(&world, NegativeReason::SharedSource);
    assert_eq!(shared.len(), 2);
    assert_eq!(shared.iter().filter(|c| c.to.as_str() == AGENT).count(), 1);
    for control in shared {
        assert_eq!(control.tier, Tier::Structural);
        let at = control.at.unwrap_or_else(|| panic!("no place"));
        assert_eq!(Some(at.exchange), control.reader_exchange);
        assert!(!text_at(&world, &at).is_empty());
    }
    assert!(!world.labels().iter().any(|l| matches!(
        l,
        Label::NegativeControl(c)
            if c.fields().source.path() == "/simulations/0/messages/11"
    )));
}

#[test]
fn labels_are_numbered_in_truth_order_with_left_out_rows_skipped() {
    let world = world(FILE, 0);
    let ids: Vec<String> = world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(t) => Some(t.fields().id.to_string()),
            Label::NegativeControl(c) => Some(c.fields().id.to_string()),
            _ => None,
        })
        .collect();
    // Records 0 (greeting), 1, 2, 3, 5, 6, 8, 9, 10, 11 (left out), 12 (no
    // reader).
    assert_eq!(
        ids,
        vec!["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8"]
    );
}

#[test]
fn a_turn_copied_from_the_scenario_is_not_labelled() {
    let world = world(FILE, 1);
    let labels = positives(&world);
    // Only the agent's answer: the user's turn relays its instructions,
    // so it is boilerplate, like the greeting.
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].from.as_str(), AGENT);
    let boilerplate = controls(&world, NegativeReason::Boilerplate);
    assert_eq!(boilerplate.len(), 2);
    assert!(
        boilerplate
            .iter()
            .any(|c| c.from.as_str() == USER && c.to.as_str() == AGENT)
    );
}

#[test]
fn a_solo_agent_has_no_peer_and_no_labels() {
    let world = world(SOLO, 0);
    assert_eq!(world.decl().agents.len(), 1);
    assert_eq!(world.decl().agents[0].key.as_str(), AGENT);
    assert_eq!(world.exchanges().len(), 2);
    assert!(
        world
            .labels()
            .iter()
            .all(|l| matches!(l, Label::ExchangeAgent(_)))
    );
    let first = request(&world, &world.exchanges()[0]);
    let prompt = first[0].part_text(0).unwrap_or_default();
    assert!(prompt.contains("<ticket>\nThe customer has no mobile data.\n</ticket>"));
}

#[test]
fn an_agent_turn_its_policy_dictates_is_boilerplate() {
    let mut results = load_results(&root(), Path::new(FILE)).unwrap_or_else(|e| panic!("{e}"));
    results.info.environment_info.policy = "# Airline Policy\nWhen asked about bags, say:\n  Each economy passenger on reservation FIX456 may bring\n  one checked bag at no cost.\n".into();
    let world = convert_simulation(&results, FILE, 1).unwrap_or_else(|e| panic!("{e}"));
    assert!(positives(&world).is_empty());
    let boilerplate = controls(&world, NegativeReason::Boilerplate);
    assert_eq!(boilerplate.len(), 3);
    assert!(boilerplate.iter().any(|c| c.from.as_str() == AGENT
        && c.text.as_deref().is_some_and(|t| t.contains("checked bag"))));
}

// --- the source ------------------------------------------------------------

fn worlds(options: &Options) -> Vec<String> {
    let mut source = source(&root(), options).unwrap_or_else(|e| panic!("{e}"));
    source
        .worlds()
        .map(|w| {
            w.map(|w| w.key().to_string())
                .unwrap_or_else(|e| panic!("{e}"))
        })
        .collect()
}

#[test]
fn files_are_discovered_sorted_and_filtered() {
    let files: Vec<String> = discover(&root(), &Options::default())
        .unwrap_or_else(|e| panic!("{e}"))
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, vec![FILE, SOLO]);
    let missing = root().join("missing");
    assert!(discover(&missing, &Options::default()).is_err());
}

#[test]
fn the_source_yields_one_world_per_simulation() {
    assert_eq!(
        worlds(&Options::default()),
        vec![
            "fixture-agent_airline_default_fixture-user_1trials/0",
            "fixture-agent_airline_default_fixture-user_1trials/1",
            "fixture-agent_telecom_no-user_fixture-user_1trials/0",
        ]
    );
    // A limit is spread over the files.
    assert_eq!(
        worlds(&Options {
            limit: Some(2),
            include: vec![]
        }),
        vec![
            "fixture-agent_airline_default_fixture-user_1trials/0",
            "fixture-agent_telecom_no-user_fixture-user_1trials/0",
        ]
    );
    assert_eq!(
        worlds(&Options {
            limit: Some(1),
            include: vec![]
        }),
        vec!["fixture-agent_airline_default_fixture-user_1trials/0"]
    );
    assert_eq!(
        worlds(&Options {
            limit: None,
            include: vec!["telecom".into()]
        }),
        vec!["fixture-agent_telecom_no-user_fixture-user_1trials/0"]
    );
}

#[test]
fn the_source_records_files_and_skips_files_the_filter_drops() {
    let mut all = source(&root(), &Options::default()).unwrap();
    assert_eq!(all.worlds().count(), 3);
    assert_eq!(all.files_read().len(), 2);
    let mut one = source(&root(), &Options::default()).unwrap();
    let kept = "fixture-agent_airline_default_fixture-user_1trials/1";
    one.select(&WorldFilter::Only([WorldKey::new(kept).unwrap()].into()));
    let keys: Vec<String> = one.worlds().map(|w| w.unwrap().key().to_string()).collect();
    assert_eq!(keys, vec![kept]);
    assert_eq!(one.files_read().len(), 1);
}

#[test]
fn options_record_the_selection() {
    assert!(Options::default().settings().is_empty());
    let options = Options {
        limit: Some(200),
        include: vec!["airline".into()],
    };
    assert_eq!(
        options.settings(),
        BTreeMap::from([
            ("limit".to_owned(), Setting::Int(200)),
            ("include[0]".to_owned(), Setting::Text("airline".into())),
        ])
    );
}

#[test]
fn the_fixtures_export() {
    let out = tempfile::tempdir().unwrap();
    let mut source = source(&root(), &Options::default()).unwrap_or_else(|e| panic!("{e}"));
    let info = ManifestInfo {
        dataset: DatasetId::new(DATASET).unwrap(),
        dataset_version: VERSION,
        source: Source {
            path: "tau2-bench/data/tau2/results/final".into(),
            revision: String::new(),
            digest: Digest::from_bytes([0; 32]),
        },
        converter: Converter {
            version: "test".into(),
            git: "test".into(),
        },
        selection: Options::default().settings(),
        pace: BTreeMap::new(),
    };
    let exported =
        export(&mut source, out.path(), info, &Split::Unsplit).unwrap_or_else(|e| panic!("{e}"));
    assert!(exported.failures.is_empty());
    assert_eq!(exported.manifest.worlds.len(), 3);
}
