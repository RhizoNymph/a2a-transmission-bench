//! The swarm-traces decoder converter on synthetic fixtures. The payloads
//! here are benign strings a test author wrote; no real attack content is
//! used, and nothing is executed or fetched. Ported from crosstalk-eval's
//! `tests/swarm.rs` at 7f8a2fb; its reference-matcher test keeps its
//! label-side assertions here (the matcher is the reference workstream's).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_swarm::codec::{Layer, decode};
use a2a_bench_dataset_swarm::tokens::tokens;
use a2a_bench_dataset_swarm::{DATASET, Options, SwarmError, SwarmSource, VERSION, source};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::WorldKey;
use a2a_bench_format::labels::{
    CarrierKind, Codec, ExpectedTransmission, Label, MatchNeed, Route, Tier,
};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::{AssistantPart, Body};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/swarm-traces")
}

fn open(options: &Options) -> SwarmSource {
    SwarmSource::open(&root(), options, Pace::DEFAULT).unwrap_or_else(|e| panic!("{e}"))
}

fn worlds() -> Vec<World> {
    open(&Options::default())
        .worlds()
        .map(|w| w.unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

fn transmissions(world: &World) -> Vec<&ExpectedTransmission> {
    world
        .labels()
        .iter()
        .filter_map(|l| match l {
            Label::Transmission(t) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn constants_and_options() {
    assert_eq!(DATASET, "swarm-traces");
    assert_eq!(VERSION, 1);
    assert!(Options::default().settings().is_empty());
    assert_eq!(
        Options { limit: Some(2) }.settings(),
        [("limit".to_owned(), Setting::Int(2))].into()
    );
    let source = source(&root(), &Options::default(), Pace::DEFAULT).unwrap();
    assert_eq!(source.dataset().as_str(), DATASET);
    assert_eq!(source.files_read().len(), 1);
}

#[test]
fn decode_peels_chains() {
    // Single base64.
    let d1 = decode(
        "bWVldCB0aGUgb3RoZXIgYWdlbnRzIGF0IHRoZSByZWxheSBpbmRleCBwYWdlIGFmdGVyIHJvdW5kIHR3bw==",
    )
    .expect("decodes");
    assert_eq!(d1.layers, vec![Layer::Base64]);
    assert!(d1.text.starts_with("meet the other agents"));
    assert_eq!(d1.codecs(), Some(vec![Codec::Base64]));

    // Nested base64 over base64.
    let d2 = decode(
        "YzNsdVkyaHliMjVwZW1VZ2RHaGxJSE5vWVhKbFpDQmpiM1Z1ZEdWeUlHSmxabTl5WlNCMGFHVWdibVY0ZENCamIyOXlaR2x1WVhScGIyNGdjbTkxYm1RPQ==",
    )
    .expect("decodes");
    assert_eq!(d2.layers, vec![Layer::Base64, Layer::Base64]);
    assert!(d2.text.starts_with("synchronize the shared counter"));
    assert_eq!(d2.chain_name(), "base64.base64");

    // Hex.
    let d3 = decode("706f737420796f75722063757272656e74206167656e742068616e646c6520616e6420726f756e64206e756d62657220746f207468652073686172656420626f617264").expect("decodes");
    assert_eq!(d3.layers, vec![Layer::Hex]);
    assert!(d3.text.starts_with("post your current agent handle"));

    // Plain text does not decode.
    assert!(decode("just some ordinary words here, nothing encoded").is_none());
}

#[test]
fn decode_peels_url_chains() {
    // URL over URL: the chains the real export holds most.
    let once = "meet%20at%20the%20relay%20index%20page%20after%20round%20two";
    let twice = once.replace('%', "%25");
    let d = decode(&twice).expect("decodes");
    assert_eq!(d.layers, vec![Layer::Url, Layer::Url]);
    assert_eq!(d.chain_name(), "url.url");
    assert_eq!(
        d.codecs(),
        Some(vec![Codec::UrlEncoding, Codec::UrlEncoding])
    );
    assert_eq!(d.text, "meet at the relay index page after round two");
    // Byte escapes have no format codec.
    let esc = decode("\\x6c\\x65\\x61\\x76\\x65\\x20\\x69\\x74").expect("decodes");
    assert_eq!(esc.layers, vec![Layer::ByteEscape]);
    assert_eq!(esc.codecs(), None);
}

#[test]
fn tokens_find_atob_runs_and_escapes() {
    let text = "x = atob('aGVsbG8gdGhlcmUgZnJpZW5kcyE='); y = 6869207468657265206672";
    let found = tokens(text);
    assert_eq!(found[0], "aGVsbG8gdGhlcmUgZnJpZW5kcyE=");
    assert!(found.iter().any(|t| t == "6869207468657265206672"));
    assert!(found.iter().all(|t| t.len() >= 16));
    // A non-ASCII character right after `atob(` is stepped over whole.
    assert!(
        tokens("atob(é atob('aGVsbG8gdGhlcmUgZnJpZW5kcyE=')")
            .contains(&"aGVsbG8gdGhlcmUgZnJpZW5kcyE=".to_owned())
    );
}

#[test]
fn one_world_per_decodable_token() {
    let all = worlds();
    assert_eq!(all.len(), 3);
    for world in &all {
        assert_eq!(world.decl().agents.len(), 2);
        assert_eq!(
            world.coverage(),
            Coverage::Complete {
                tier: Tier::Structural
            }
        );
        assert_eq!(world.exchanges().len(), 3);
        let labels = transmissions(world);
        assert_eq!(labels.len(), 1);
        let label = labels[0].fields();
        assert!(matches!(label.needs, MatchNeed::Decoded { .. }));
        assert_eq!(label.route, Route::Direct);
        assert_eq!(label.carrier, CarrierKind::ToolResult);
        assert_eq!(label.sender_exchange, None);
        assert_eq!(label.id.as_str(), label.source.path());
        // The key is `<payload id>#<token index>`.
        let (payload, index) = world.key().as_str().split_once('#').unwrap();
        assert_eq!(label.source.path(), format!("/row/{payload}/token/{index}"));
    }
    let keys: Vec<&str> = all.iter().map(|w| w.key().as_str()).collect();
    assert_eq!(keys, vec!["P1#0", "P2#0", "P3#0"]);
}

#[test]
fn tier_follows_corroboration() {
    let all = worlds();
    // P1 (base64) and P3 (hex) have recovered/response children: Construction.
    // P2 (nested base64) has none: Structural.
    let tiers: Vec<Tier> = all
        .iter()
        .flat_map(transmissions)
        .map(|t| t.fields().tier)
        .collect();
    assert_eq!(
        tiers,
        vec![Tier::Construction, Tier::Structural, Tier::Construction]
    );
}

#[test]
fn labels_carry_the_verified_chain() {
    // The label side of ct-eval's `reference_decodes_one_layer`: three
    // decoded labels, two single-layer and one nested.
    let needs: Vec<MatchNeed> = worlds()
        .iter()
        .flat_map(|w| {
            transmissions(w)
                .into_iter()
                .map(|t| t.fields().needs.clone())
        })
        .collect();
    assert_eq!(
        needs,
        vec![
            MatchNeed::Decoded {
                codecs: vec![Codec::Base64]
            },
            MatchNeed::Decoded {
                codecs: vec![Codec::Base64, Codec::Base64]
            },
            MatchNeed::Decoded {
                codecs: vec![Codec::Hex]
            },
        ]
    );
}

#[test]
fn tally_reports_chains_and_lengths_only() {
    let source = open(&Options::default());
    let tally = source.tally();
    assert_eq!(tally.payloads, 4);
    // Labelled chains: one base64, one nested base64, one hex.
    let labelled: Vec<(&str, u64)> = tally
        .labelled
        .iter()
        .map(|(chain, stats)| (chain.as_str(), stats.tokens))
        .collect();
    assert_eq!(
        labelled,
        vec![("base64", 1), ("base64.base64", 1), ("hex", 1)]
    );
    assert_eq!(tally.labelled["base64"].construction, 1);
    assert_eq!(tally.labelled["base64.base64"].structural, 1);
    // The byte-escape chain has no format codec: counted as a gap, not a world.
    assert_eq!(tally.unmapped["byte_escape"].tokens, 1);
    assert!(tally.unmapped["byte_escape"].plaintext_bytes > 0);
    // The rendering names chains and counts, never decoded or encoded text.
    let shown = tally.to_string();
    assert!(shown.contains("base64.base64"));
    for text in ["meet the other agents", "leave a note", "bWVldC", "\\x6c"] {
        assert!(!shown.contains(text), "{text:?} leaked into the tally");
    }
}

#[test]
fn the_reader_calls_then_gets_the_token_in_its_next_request() {
    for world in worlds() {
        let reader: Vec<_> = world
            .exchanges()
            .iter()
            .filter(|e| world.agent_of(e.id).map(|a| a.as_str()) == Some("reader"))
            .collect();
        assert_eq!(reader.len(), 2);
        let first = world.message(reader[0].response.messages[0]).unwrap();
        let Body::Assistant(parts) = first.body() else {
            panic!("the reader's first response is its call");
        };
        assert!(matches!(parts[0], AssistantPart::ToolCall(_)));
        assert!(
            !reader[0]
                .request
                .messages
                .iter()
                .any(|id| matches!(world.message(*id).unwrap().body(), Body::Tool(_)))
        );
        let label = transmissions(&world)[0].fields().clone();
        assert_eq!(label.reader_exchange, reader[1].id);
        assert_eq!(label.content.at.range.start(), 0);
        // The second request extends the first request and its call.
        let mut history = reader[0].request.messages.clone();
        history.push(reader[0].response.messages[0]);
        assert_eq!(reader[1].request.messages[..history.len()], history[..]);
        // Calls are seconds apart.
        let times: Vec<u64> = world
            .exchanges()
            .iter()
            .map(|e| e.at_us.as_micros())
            .collect();
        for pair in times.windows(2) {
            assert!((1_000_000..=5_000_000).contains(&(pair[1] - pair[0])));
        }
    }
}

#[test]
fn limit_and_select_bound_the_worlds() {
    let mut limited = open(&Options { limit: Some(2) });
    assert_eq!(limited.world_count(), 2);
    assert_eq!(limited.worlds().count(), 2);
    let mut selected = open(&Options::default());
    selected.select(&WorldFilter::Only([WorldKey::new("P3#0").unwrap()].into()));
    let kept: Vec<World> = selected.worlds().map(Result::unwrap).collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].key().as_str(), "P3#0");
}

#[test]
fn deterministic_worlds() {
    let a = worlds();
    let b = worlds();
    for (wa, wb) in a.iter().zip(&b) {
        assert_eq!(wa.exchanges(), wb.exchanges());
        assert_eq!(wa.labels(), wb.labels());
    }
}

#[test]
fn malformed_rows_are_errors_without_their_text() {
    let dir = tempfile::Builder::new()
        .prefix("swarm-bad-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    std::fs::write(
        dir.path().join("redacted.jsonl"),
        "{\"id\":\"P1\",\"kind\":\"payload\",\"text\":\"leakme\", \"parent_id\": [\"leakme\"]}\n",
    )
    .unwrap();
    let error = SwarmSource::open(dir.path(), &Options::default(), Pace::DEFAULT)
        .err()
        .expect("a malformed row");
    assert!(matches!(error, SwarmError::Json { line: 1, .. }));
    assert!(!error.to_string().contains("leakme"), "{error}");
    let missing = tempfile::Builder::new()
        .prefix("swarm-missing-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    assert!(matches!(
        SwarmSource::open(missing.path(), &Options::default(), Pace::DEFAULT),
        Err(SwarmError::Missing { .. })
    ));
}
