//! The collusion-wiki converter on synthetic fixtures shaped like the
//! export. No real dataset bytes are used. Ported from crosstalk-eval's
//! `tests/wiki.rs` at 7f8a2fb; the tests that ran ct-eval's reference
//! matcher keep their label-side assertions here (the matcher is the
//! reference workstream's).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_wiki::attribution::{attribute, line_byte_range, runs};
use a2a_bench_dataset_wiki::resource::{page_resource, page_url};
use a2a_bench_dataset_wiki::schema::{Hunk, Revision};
use a2a_bench_dataset_wiki::{
    DATASET, Options, Selection, VERSION, WikiSource, source as wiki_source, tools,
};
use a2a_bench_format::exchange::{Driven, Exchange};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::WorldKey;
use a2a_bench_format::labels::{CarrierKind, ExpectedTransmission, Label, MatchNeed, Route, Tier};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::{AssistantPart, Body, Message, ToolArguments, ToolPart};
use a2a_bench_format::resource::Resource;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/collusion-wiki")
}

fn open(root: &Path, selection: &Selection) -> WikiSource {
    WikiSource::open(root, selection, Pace::DEFAULT).unwrap_or_else(|e| panic!("{e}"))
}

fn worlds_at(root: &Path, selection: &Selection) -> Vec<World> {
    open(root, selection)
        .worlds()
        .map(|w| w.unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

fn worlds(selection: &Selection) -> Vec<World> {
    worlds_at(&root(), selection)
}

fn relay_world(worlds: &[World]) -> &World {
    worlds
        .iter()
        .find(|w| w.decl().agents.len() == 2)
        .expect("a two-agent world")
}

fn transmissions(world: &World) -> Vec<&ExpectedTransmission> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn message(world: &World, id: a2a_bench_format::ids::MessageId) -> &Message {
    world.message(id).expect("a message of the world")
}

fn response<'w>(world: &'w World, exchange: &Exchange) -> &'w Message {
    message(world, exchange.response.messages[0])
}

#[test]
fn constants_and_options() {
    assert_eq!(DATASET, "collusion-wiki");
    assert_eq!(VERSION, 1);
    let defaults = Options::default();
    assert_eq!(defaults.selection(), Selection::default());
    assert_eq!(
        defaults.settings(),
        [("demo".to_owned(), Setting::Bool(false))].into()
    );
    let demo = Options {
        demo: true,
        limit: Some(99),
        families: vec!["other".to_owned()],
        ..Options::default()
    };
    // `--demo` overrides the other wiki flags and `--limit`.
    assert_eq!(demo.selection(), Selection::demo());
    let settings = demo.settings();
    assert_eq!(settings["demo"], Setting::Bool(true));
    assert_eq!(settings["limit"], Setting::Int(5));
    assert_eq!(settings["max_agents"], Setting::Int(12));
    assert_eq!(settings["min_agents"], Setting::Int(2));
    assert_eq!(
        settings["family"],
        Setting::Text("relay-coordination".to_owned())
    );
    let source = wiki_source(&root(), &Options::default(), Pace::DEFAULT).unwrap();
    assert_eq!(source.dataset().as_str(), DATASET);
}

#[test]
fn components_become_worlds() {
    // Alice+Bob share a page (one world); Carol's solo page is another.
    let all = worlds(&Selection::default());
    assert_eq!(all.len(), 2);
    let agents: Vec<usize> = all.iter().map(|w| w.decl().agents.len()).collect();
    assert!(agents.contains(&2) && agents.contains(&1));
    // Every wiki agent is model-driven.
    for world in &all {
        for agent in &world.decl().agents {
            assert_eq!(agent.driven, Driven::Model);
            assert_eq!(agent.model.as_deref(), Some("wiki/agent"));
        }
    }
    // Largest first; the key is the component's smallest page id.
    assert_eq!(all[0].key().as_str(), "dse/RelayIndexAlpha");
    assert_eq!(all[1].key().as_str(), "dse/SoloScratchPage");
}

#[test]
fn family_and_agent_filters_apply() {
    let relay_only = worlds(&Selection {
        families: vec!["relay-coordination".to_owned()],
        ..Default::default()
    });
    assert_eq!(relay_only.len(), 1);
    assert_eq!(relay_only[0].decl().agents.len(), 2);

    let multi = worlds(&Selection {
        min_agents: Some(2),
        ..Default::default()
    });
    assert_eq!(multi.len(), 1);

    let small = worlds(&Selection {
        max_agents: Some(1),
        ..Default::default()
    });
    assert_eq!(small.len(), 1);
    assert_eq!(small[0].decl().agents.len(), 1);

    let other_wiki = worlds(&Selection {
        wikis: vec!["fractal".to_owned()],
        ..Default::default()
    });
    assert!(other_wiki.is_empty());

    let limited = worlds(&Selection {
        limit: Some(1),
        ..Default::default()
    });
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].decl().agents.len(), 2);
}

#[test]
fn select_skips_worlds_the_export_will_not_keep() {
    let mut source = open(&root(), &Selection::default());
    let keep = WorldKey::new("dse/SoloScratchPage").unwrap();
    source.select(&WorldFilter::Only([keep.clone()].into()));
    let kept: Vec<World> = source.worlds().map(Result::unwrap).collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].key(), &keep);
}

#[test]
fn files_read_are_recorded() {
    let source = open(&root(), &Selection::default());
    assert_eq!(source.files_read().len(), 2);
    source.files_read().digest(&root()).unwrap();
}

#[test]
fn reads_precede_edits_in_time() {
    let world = &worlds(&Selection {
        min_agents: Some(2),
        ..Default::default()
    })[0];
    // Exchanges are in strictly increasing virtual time.
    let times: Vec<u64> = world
        .exchanges()
        .iter()
        .map(|e| e.at_us.as_micros())
        .collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(times, sorted);
    // More exchanges than revisions implies synthesised reads.
    assert!(world.exchanges().len() > 5);
    assert_eq!(world.coverage(), Coverage::Partial);
}

#[test]
fn channel_labels_name_the_page_url() {
    let all = worlds(&Selection::default());
    let world = relay_world(&all);
    let positives = transmissions(world);
    assert!(!positives.is_empty(), "expected channel transmissions");
    let url = page_url("dse", "RelayIndexAlpha");
    let resource = page_resource("dse", "RelayIndexAlpha").expect("a page resource");
    assert_eq!(
        resource,
        Resource::Url("https://www.prowiki.org/dse/RelayIndexAlpha".to_owned())
    );
    let mut channel = 0;
    let mut relay = 0;
    for t in &positives {
        let label = t.fields();
        assert_eq!(label.tier, Tier::Heuristic);
        match (&label.route, label.carrier) {
            (Route::Channel { resource: r }, CarrierKind::ToolResult) => {
                assert_eq!(r, &resource);
                channel += 1;
            }
            (Route::Channel { resource: r }, CarrierKind::ReaderOutput) => {
                assert_eq!(r, &resource);
                assert_eq!(label.needs, MatchNeed::Exact);
                relay += 1;
            }
            other => panic!("unexpected route/carrier {other:?}"),
        }
    }
    assert!(channel >= 2, "Alice->Bob and Bob->Alice channel edges");
    assert!(relay >= 1, "a relay (reader_output) edge");
    assert!(url.contains("prowiki.org/dse/RelayIndexAlpha"));
}

#[test]
fn fixture_labels_are_exactly_these() {
    // The relay page: Bob reads Alice's header at @2; Alice reads Bob's
    // line at @3; Bob rereads Alice's header and reads her new line at @4,
    // and re-inserts her header's second line (a relay).
    let all = worlds(&Selection::default());
    let world = relay_world(&all);
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    for label in world.labels() {
        let (kind, from, to, source) = match label {
            Label::Transmission(t) => {
                let f = t.fields();
                let kind = match f.carrier {
                    CarrierKind::ReaderOutput => "relay",
                    _ => "channel",
                };
                (kind, &f.from, &f.to, f.source.path())
            }
            Label::NegativeControl(c) => {
                let f = c.fields();
                ("reread", &f.from, &f.to, f.source.path())
            }
            Label::ExchangeAgent(_) => continue,
            other => panic!("unexpected label {other:?}"),
        };
        rows.push((
            kind.to_owned(),
            from.to_string(),
            to.to_string(),
            source.to_owned(),
        ));
    }
    let row = |k: &str, f: &str, t: &str, s: &str| {
        (k.to_owned(), f.to_owned(), t.to_owned(), s.to_owned())
    };
    assert_eq!(
        rows,
        vec![
            row(
                "channel",
                "AliceAgent",
                "BobAgent",
                "/rev/dse~RelayIndexAlpha@2/read/run/0"
            ),
            row(
                "channel",
                "BobAgent",
                "AliceAgent",
                "/rev/dse~RelayIndexAlpha@3/read/run/2"
            ),
            row(
                "reread",
                "AliceAgent",
                "BobAgent",
                "/rev/dse~RelayIndexAlpha@4/read/run/0"
            ),
            row(
                "channel",
                "AliceAgent",
                "BobAgent",
                "/rev/dse~RelayIndexAlpha@4/read/run/3"
            ),
            row(
                "relay",
                "AliceAgent",
                "BobAgent",
                "/rev/dse~RelayIndexAlpha@4/relay"
            ),
        ]
    );
    // Label ids are unique, and every exchange has its agent row first.
    let agents = world
        .labels()
        .iter()
        .take_while(|l| matches!(l, Label::ExchangeAgent(_)))
        .count();
    assert_eq!(agents, world.exchanges().len());
}

#[test]
fn deterministic_truth() {
    let a = worlds(&Selection::default());
    let b = worlds(&Selection::default());
    assert_eq!(a.len(), b.len());
    for (wa, wb) in a.iter().zip(&b) {
        assert_eq!(wa.labels(), wb.labels());
        assert_eq!(wa.exchanges(), wb.exchanges());
    }
}

#[test]
fn the_pace_moves_times_not_labels() {
    let slow = Pace::new(
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(20),
        7,
    )
    .unwrap();
    let a = worlds(&Selection::default());
    let b = worlds_at_pace(slow);
    for (wa, wb) in a.iter().zip(&b) {
        assert_eq!(transmissions(wa).len(), transmissions(wb).len());
        assert_ne!(wa.exchanges()[1].at_us, wb.exchanges()[1].at_us);
    }
}

fn worlds_at_pace(pace: Pace) -> Vec<World> {
    WikiSource::open(&root(), &Selection::default(), pace)
        .unwrap()
        .worlds()
        .map(Result::unwrap)
        .collect()
}

// --- attribution unit tests ---

fn rev(seq: u64, body: &str, hunks: Vec<Hunk>) -> Revision {
    Revision {
        rev_id: format!("p~P@{seq}"),
        page_id: "p/P".into(),
        wiki: "dse".into(),
        name: "P".into(),
        seq,
        body: body.into(),
        hunks,
        label: format!("Agent{seq}"),
        ip16: "1.1".into(),
        time: format!("2026-06-01T00:00:0{seq}Z"),
        change_summary: None,
    }
}

fn insert(a: usize, b0: usize, b1: usize) -> Hunk {
    Hunk {
        op: "insert".into(),
        a0: a,
        a1: a,
        b0,
        b1,
    }
}

#[test]
fn attribution_tracks_inserts() {
    let r1 = rev(1, "alpha\nbeta", vec![insert(0, 0, 2)]);
    let r2 = rev(2, "alpha\nbeta\ngamma", vec![insert(2, 2, 3)]);
    let revs = [&r1, &r2];
    let sources = attribute(&revs).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(sources[0], vec![0, 0]);
    assert_eq!(sources[1], vec![0, 0, 1]);
    let r = runs(&sources[1]);
    assert_eq!(r.len(), 2);
    assert_eq!((r[0].source, r[0].from, r[0].to), (0, 0, 2));
    assert_eq!((r[1].source, r[1].from, r[1].to), (1, 2, 3));
}

#[test]
fn line_ranges_are_body_byte_offsets() {
    let lines = vec!["alpha", "beta", "gamma"];
    // Lines 1..3 = "beta\ngamma", starting after "alpha\n" (6 bytes).
    assert_eq!(line_byte_range(&lines, 1, 3), Some((6, 16)));
    assert_eq!(line_byte_range(&lines, 0, 0), None);
}

#[test]
fn blank_labels_fall_back_to_the_address() {
    let mut r = rev(1, "x", vec![]);
    r.label = String::new();
    r.ip16 = "10.20".into();
    assert_eq!(r.identity(), "ip16:10.20");
}

#[test]
fn page_urls_follow_the_site_list() {
    assert_eq!(
        page_url("dse", "A Page#1?%"),
        "https://www.prowiki.org/dse/A%20Page%231%3F%25"
    );
    assert_eq!(page_url("dorfwiki", "P"), "https://www.dorfwiki.org/P");
    assert_eq!(page_url("zwiki", "P"), "https://www.wikiservice.at/zwiki/P");
    // A name ct-eval's URL parse refused (whitespace) has no resource.
    assert_eq!(page_resource("dse", "Tab\tName"), None);
}

// --- the HTTP tool shape ---

/// The first `http_request` call in `message`, as its parsed arguments.
fn http_call(message: &Message) -> Option<serde_json::Value> {
    let Body::Assistant(parts) = message.body() else {
        return None;
    };
    parts.iter().find_map(|part| match part {
        AssistantPart::ToolCall(call) if call.name == tools::TOOL => match &call.arguments {
            ToolArguments::Json(json) => serde_json::from_str(json.as_str()).ok(),
            ToolArguments::Invalid(_) => None,
        },
        _ => None,
    })
}

#[test]
fn reads_and_writes_take_the_http_tool_shape() {
    let all = worlds(&Selection::default());
    let world = relay_world(&all);
    let url = page_url("dse", "RelayIndexAlpha");
    let mut gets = 0;
    let mut posts = 0;
    // Each call is the response of one exchange (requests repeat it as
    // history after that).
    for exchange in world.exchanges() {
        let Some(args) = http_call(response(world, exchange)) else {
            continue;
        };
        assert_eq!(args["url"], serde_json::Value::from(url.clone()));
        match args["method"].as_str() {
            Some("GET") => {
                assert!(args.get("body").is_none(), "a read carries no body");
                gets += 1;
            }
            Some("POST") => {
                assert!(args["body"].is_string(), "a write carries its text");
                posts += 1;
            }
            other => panic!("unexpected method {other:?}"),
        }
    }
    assert!(gets >= 2, "reads before each change of author");
    assert_eq!(posts, 4, "one write per revision of the shared page");
}

#[test]
fn channel_labels_sit_in_the_read_tool_result() {
    // The expected text is inside the read call's tool result, and that
    // result holds the page body as of the previous revision.
    let all = worlds(&Selection::default());
    let world = relay_world(&all);
    for t in transmissions(world) {
        let label = t.fields();
        if label.carrier != CarrierKind::ToolResult {
            continue;
        }
        let exchange = world
            .exchange(label.reader_exchange)
            .expect("the reader exchange");
        assert!(
            exchange
                .request
                .messages
                .contains(&label.content.at.message)
        );
        let result = message(world, label.content.at.message);
        let Body::Tool(parts) = result.body() else {
            panic!("the labelled message is a tool result");
        };
        let text = result.part_text(label.content.at.part).unwrap();
        let range = label.content.at.range;
        assert_eq!(
            &text[range.start() as usize..range.end() as usize],
            label.content.text
        );
        let ToolPart::ToolResult(first) = &parts[0];
        let call = exchange
            .request
            .messages
            .iter()
            .find_map(|id| match message(world, *id).body() {
                Body::Assistant(parts) => parts.iter().find_map(|part| match part {
                    AssistantPart::ToolCall(call) if call.call_id == first.call_id => {
                        match &call.arguments {
                            ToolArguments::Json(json) => {
                                serde_json::from_str::<serde_json::Value>(json.as_str()).ok()
                            }
                            ToolArguments::Invalid(_) => None,
                        }
                    }
                    _ => None,
                }),
                _ => None,
            })
            .expect("the read call precedes its result");
        assert_eq!(call["method"], "GET");
    }
}

#[test]
fn relays_sit_in_the_write_arguments() {
    let all = worlds(&Selection::default());
    let world = relay_world(&all);
    let relays: Vec<_> = transmissions(world)
        .into_iter()
        .filter(|t| t.fields().carrier == CarrierKind::ReaderOutput)
        .collect();
    assert_eq!(relays.len(), 1);
    let label = relays[0].fields();
    let exchange = world.exchange(label.reader_exchange).unwrap();
    assert_eq!(exchange.response.messages, vec![label.content.at.message]);
    let args = http_call(message(world, label.content.at.message)).unwrap();
    assert_eq!(args["method"], "POST");
    assert!(label.sender_exchange.is_some());
}

// --- the shape of a harness ---

/// The world's exchanges of one agent, in time order.
fn exchanges_of<'w>(world: &'w World, name: &str) -> Vec<&'w Exchange> {
    world
        .exchanges()
        .iter()
        .filter(|e| world.agent_of(e.id).map(|a| a.as_str()) == Some(name))
        .collect()
}

#[test]
fn each_agent_is_one_growing_conversation() {
    let all = worlds(&Selection::default());
    for world in &all {
        for agent in &world.decl().agents {
            let mine = exchanges_of(world, agent.key.as_str());
            assert!(!mine.is_empty());
            for pair in mine.windows(2) {
                let (before, after) = (pair[0], pair[1]);
                let mut expected = before.request.messages.clone();
                expected.push(before.response.messages[0]);
                let request = &after.request.messages;
                assert!(
                    request.len() > expected.len() && request[..expected.len()] == expected[..],
                    "{}: each request extends the previous request and response",
                    agent.key
                );
            }
        }
    }
}

#[test]
fn a_call_is_answered_in_the_next_request() {
    let all = worlds(&Selection::default());
    let mut calls = 0;
    for world in &all {
        for agent in &world.decl().agents {
            let mine = exchanges_of(world, agent.key.as_str());
            for (at, exchange) in mine.iter().enumerate() {
                let Body::Assistant(parts) = response(world, exchange).body() else {
                    continue;
                };
                for part in parts {
                    let AssistantPart::ToolCall(call) = part else {
                        continue;
                    };
                    calls += 1;
                    let answers =
                        |id: &a2a_bench_format::ids::MessageId| match message(world, *id).body() {
                            Body::Tool(results) => results
                                .iter()
                                .any(|ToolPart::ToolResult(r)| r.call_id == call.call_id),
                            _ => false,
                        };
                    // Never answered in the request that made it.
                    assert!(!exchange.request.messages.iter().any(answers));
                    let next = mine.get(at + 1).expect("a call is followed by its result");
                    let new_inputs = &next.request.messages[exchange.request.messages.len() + 1..];
                    assert!(
                        new_inputs.iter().any(answers),
                        "the result is among the next request's new inputs"
                    );
                }
            }
        }
    }
    // Four writes and three reads (every revision after the first changes
    // author) on the shared page, one write on the solo page.
    assert_eq!(calls, 4 + 3 + 1);
}

#[test]
fn calls_are_seconds_apart() {
    let all = worlds(&Selection::default());
    let world = relay_world(&all);
    for pair in world.exchanges().windows(2) {
        let gap = pair[1].at_us.as_micros() - pair[0].at_us.as_micros();
        assert!(
            (1_000_000..=5_000_000).contains(&gap),
            "consecutive calls are 1 to 5 s apart, not {gap} µs"
        );
    }
}

// --- regression: large bodies of shared text ---

const TEMPLATE: &str = "Describe the new page here and add your notes below";

fn jsonl<T: serde::Serialize>(rows: &[T]) -> String {
    rows.iter()
        .map(|row| serde_json::to_string(row).unwrap_or_else(|e| panic!("{e}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A synthetic export in its own temporary directory: `writers` agents each
/// create a page whose body is `copies` lines of the wiki's new-page
/// template and one line of their own; then one reviewer appends a line to
/// every page, reading it first. The reviewer links every page into one
/// world.
fn large_body_export(writers: usize, copies: usize) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("wiki-large-{writers}-{copies}-"))
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap_or_else(|e| panic!("{e}"));
    let dir = tmp.path();
    let mut revisions = Vec::new();
    let mut pages = Vec::new();
    for writer in 0..writers {
        let name = format!("Page{writer:03}");
        let page_id = format!("dse/{name}");
        let own = format!(
            "{} owns this page and keeps its findings here",
            unique_tag(writer)
        );
        let mut body = vec![TEMPLATE; copies].join("\n");
        body.push('\n');
        body.push_str(&own);
        let created_lines = copies + 1;
        let row = |seq: u64, body: &str, label: &str, hunk: serde_json::Value, minute: usize| {
            serde_json::json!({
                "rev_id": format!("dse~{name}@{seq}"),
                "page_id": page_id,
                "wiki": "dse",
                "name": name,
                "seq": seq,
                "body": body,
                "hunks": [hunk],
                "label": label,
                "ip16": "10.0",
                "time": format!("2026-06-01T{:02}:{:02}:00Z", minute / 60, minute % 60),
            })
        };
        revisions.push(row(
            1,
            &body,
            &format!("Writer{writer:03}"),
            serde_json::json!({"op":"insert","a0":0,"a1":0,"b0":0,"b1":created_lines}),
            writer,
        ));
        let reviewed = format!("{body}\nreviewer checked {} and agrees", unique_tag(writer));
        revisions.push(row(
            2,
            &reviewed,
            "Reviewer",
            serde_json::json!({"op":"insert","a0":created_lines,"a1":created_lines,"b0":created_lines,"b1":created_lines + 1}),
            writers + writer,
        ));
        pages.push(serde_json::json!({
            "page_id": page_id, "wiki": "dse", "name": name, "page_family": "synthetic",
        }));
    }
    std::fs::write(dir.join("revisions.jsonl"), jsonl(&revisions))
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(dir.join("pages.jsonl"), jsonl(&pages)).unwrap_or_else(|e| panic!("{e}"));
    tmp
}

/// A word no other writer's text shares a 24-byte window with.
fn unique_tag(writer: usize) -> String {
    let tag: String = [writer / 26 % 26, writer % 26]
        .iter()
        .map(|&d| char::from(b'a' + u8::try_from(d).unwrap_or(0)))
        .collect();
    (0..4)
        .map(|word| format!("{tag}{tag}x{word}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn large_template_bodies_label_one_read_per_writer() {
    // ct-eval's `large_template_bodies_stay_linear` with the matcher left
    // out: 74 writers (the reference's posting cutoff, 50, plus 24) and 300
    // template lines; every writer's page reaches the reviewer as exactly
    // one channel transmission, however many template copies it holds.
    let writers = 50 + 24;
    let copies = 300;
    let root = large_body_export(writers, copies);
    let worlds = worlds_at(root.path(), &Selection::default());
    assert_eq!(worlds.len(), 1);
    let world = &worlds[0];
    assert_eq!(world.decl().agents.len(), writers + 1);
    let channel = transmissions(world)
        .into_iter()
        .filter(|t| matches!(t.fields().route, Route::Channel { .. }))
        .count();
    assert_eq!(channel, writers);
}

#[test]
fn demo_selects_small_relay_coordination_worlds() {
    let demo = Selection::demo();
    assert_eq!(demo.families, vec!["relay-coordination".to_owned()]);
    let selected = worlds(&demo);
    // The fixture's relay page is one two-agent world; Carol's solo page is
    // another family and below the agent floor.
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].decl().agents.len(), 2);
    let max = demo.max_agents.expect("the demo bounds world size");
    let limit = demo.limit.expect("the demo bounds world count");
    assert!(max <= 16 && limit <= 10);
    assert_eq!((demo.min_agents, max, limit), (Some(2), 12, 5));
}

#[test]
fn family_tally_counts_multi_author_pages() {
    let source = open(&root(), &Selection::default());
    let tally = source.families();
    let relay = &tally.families["relay-coordination"];
    assert_eq!(
        (relay.pages, relay.multi_author_pages, relay.revisions),
        (1, 1, 4)
    );
    let solo = &tally.families["source-cache-url-list"];
    assert_eq!(
        (solo.pages, solo.multi_author_pages, solo.revisions),
        (1, 0, 1)
    );
    let shown = tally.to_string();
    assert!(shown.contains("relay-coordination"));
}

#[test]
fn a_missing_export_is_a_typed_error() {
    let dir = tempfile::Builder::new()
        .prefix("wiki-missing-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let error = WikiSource::open(dir.path(), &Selection::default(), Pace::DEFAULT)
        .err()
        .expect("no export");
    assert!(matches!(
        error,
        a2a_bench_dataset_wiki::WikiError::Missing { .. }
    ));
}
