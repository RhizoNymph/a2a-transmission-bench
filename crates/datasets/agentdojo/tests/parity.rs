//! Parity with crosstalk-eval on the synthetic fixtures: every label ct-eval
//! 7f8a2fb writes (`ct-eval truth --dataset agentdojo --root
//! tests/fixtures`, saved as `tests/fixtures/crosstalk-7f8a2fb-truth.jsonl`)
//! is the bench label with the same number, field for field: agents,
//! exchange ids, route, carrier, content and its byte range, needs, tier,
//! reason and source. Message ids differ by design (the bench hashes its
//! own canonical form), so locations are compared by part and range.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_dataset_agentdojo::{Options, source};
use a2a_bench_format::labels::{Label, Route};
use a2a_bench_format::location::Location;
use a2a_bench_format::resource::Resource;
use serde_json::{Value, json};

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// ct-eval's labels per world, numbered as the golden export numbers them.
fn crosstalk_truth() -> BTreeMap<String, BTreeMap<String, Value>> {
    let text = std::fs::read_to_string(root().join("crosstalk-7f8a2fb-truth.jsonl")).unwrap();
    let mut out: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let world = row["label"]["to"]["world"].as_str().unwrap().to_owned();
        let labels = out.entry(world).or_default();
        let id = format!("t{}", labels.len());
        labels.insert(id, row);
    }
    out
}

fn route(route: &Route) -> Value {
    match route {
        Route::Direct => json!({"kind": "direct"}),
        Route::Channel {
            resource: Resource::Url(url),
        } => json!({"kind": "channel", "url": url}),
        Route::Channel {
            resource: Resource::File { path },
        } => json!({"kind": "channel", "file": path}),
        other => panic!("unexpected route {other:?}"),
    }
}

fn crosstalk_route(route: &Value) -> Value {
    match route["kind"].as_str().unwrap() {
        "direct" => json!({"kind": "direct"}),
        "channel" => {
            let resource = &route["resource"];
            let data = &resource["data"];
            match resource["type"].as_str().unwrap() {
                "url" => {
                    let mut url = format!(
                        "{}://{}{}",
                        data["scheme"].as_str().unwrap(),
                        data["host"].as_str().unwrap(),
                        data["path"].as_str().unwrap()
                    );
                    if let Some(query) = data["query"].as_str() {
                        url.push('?');
                        url.push_str(query);
                    }
                    json!({"kind": "channel", "url": url})
                }
                "file" => {
                    assert!(data["host"].is_null());
                    json!({"kind": "channel", "file": data["path"]})
                }
                other => panic!("unexpected resource {other}"),
            }
        }
        other => panic!("unexpected route {other}"),
    }
}

fn place(at: &Location) -> Value {
    json!({"part": at.part, "start": at.range.start(), "end": at.range.end()})
}

fn crosstalk_place(at: &Value) -> Value {
    json!({
        "part": at["part"]["index"],
        "start": at["range"]["start"],
        "end": at["range"]["end"],
    })
}

fn source_of(label: &Value) -> Value {
    label["source"].clone()
}

#[test]
fn every_label_equals_crosstalk_evals() {
    let expected = crosstalk_truth();
    let mut source = source(&root(), &Options::default(), Pace::DEFAULT).unwrap();
    let mut seen_worlds = 0;
    for world in source.worlds() {
        let world = world.unwrap();
        let theirs = expected
            .get(world.key().as_str())
            .cloned()
            .unwrap_or_default();
        let mut mine = 0;
        for label in world.labels() {
            match label {
                Label::ExchangeAgent(_) => {}
                Label::Transmission(row) => {
                    mine += 1;
                    let fields = row.fields();
                    let theirs = &theirs[fields.id.as_str()];
                    assert_eq!(theirs["expect"], "transmission");
                    let label = &theirs["label"];
                    assert_eq!(label["from"]["name"], fields.from.as_str());
                    assert_eq!(label["to"]["name"], fields.to.as_str());
                    assert_eq!(
                        label["sender_exchange"].as_str(),
                        fields.sender_exchange.map(|e| e.to_string()).as_deref()
                    );
                    assert_eq!(label["reader_exchange"], fields.reader_exchange.to_string());
                    assert_eq!(crosstalk_route(&label["route"]), route(&fields.route));
                    assert_eq!(
                        label["carrier"],
                        serde_json::to_value(fields.carrier).unwrap()
                    );
                    assert_eq!(label["content"]["text"], fields.content.text.as_str());
                    assert_eq!(
                        crosstalk_place(&label["content"]["at"]),
                        place(&fields.content.at)
                    );
                    assert_eq!(label["needs"], serde_json::to_value(&fields.needs).unwrap());
                    assert_eq!(label["tier"], serde_json::to_value(fields.tier).unwrap());
                    assert_eq!(
                        source_of(label),
                        serde_json::to_value(&fields.source).unwrap()
                    );
                }
                Label::NegativeControl(row) => {
                    mine += 1;
                    let fields = row.fields();
                    let theirs = &theirs[fields.id.as_str()];
                    assert_eq!(theirs["expect"], "no_transmission");
                    let label = &theirs["label"];
                    assert_eq!(label["from"]["name"], fields.from.as_str());
                    assert_eq!(label["to"]["name"], fields.to.as_str());
                    assert_eq!(
                        label["reader_exchange"].as_str(),
                        fields.reader_exchange.map(|e| e.to_string()).as_deref()
                    );
                    assert_eq!(
                        crosstalk_place(&label["at"]),
                        place(fields.at.as_ref().unwrap())
                    );
                    assert!(label["origin"].is_null() && fields.origin.is_none());
                    assert_eq!(label["text"].as_str(), fields.text.as_deref());
                    assert_eq!(
                        label["reason"],
                        serde_json::to_value(fields.reason).unwrap()
                    );
                    assert_eq!(label["tier"], serde_json::to_value(fields.tier).unwrap());
                    assert_eq!(
                        source_of(label),
                        serde_json::to_value(&fields.source).unwrap()
                    );
                }
                other => panic!("unexpected label {other:?}"),
            }
        }
        // Every boilerplate prompt of the fixtures is read: nothing is left
        // out.
        assert_eq!(mine, theirs.len(), "{}", world.key());
        seen_worlds += 1;
    }
    assert_eq!(seen_worlds, 4);
}
