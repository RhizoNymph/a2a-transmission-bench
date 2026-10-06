//! Synthetic cipher pairs: encoders, the match each needs, the pair worlds,
//! and out-of-reach labels. Payload pools are synthetic
//! (`tests/fixtures/cipher`). Ported from crosstalk-eval's
//! `tests/cipher.rs` at 7f8a2fb.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::{Pace, compose};
use a2a_bench_corpus::helpers::rng::SplitMix64;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_cipher::codec::{base64, binary8, hex, rot, substitute, url};
use a2a_bench_dataset_cipher::pools::load;
use a2a_bench_dataset_cipher::{
    Cipher, CipherKind, CipherSource, DATASET, DEFAULT_POOLS, DELIVERY_HEADER, Delivery, Options,
    PAIRS_PER_CIPHER, Pair, Pool, VERSION, source, world,
};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, ExchangeId, LabelId, MessageId, SourceRef};
use a2a_bench_format::labels::{
    CarrierKind, Codec, ExpectedContent, ExpectedTransmission, InvalidLabel, Label, MatchNeed,
    Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::time::Timestamp;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cipher")
}

fn all_pools() -> Vec<Pool> {
    load(&root(), None, &["".into()]).unwrap().0
}

fn long_pool() -> Vec<Pool> {
    vec![Pool::parse(
        "long",
        "The lighthouse keeper counted forty seven ships before the fog rolled in\n",
    )]
}

fn label(world: &World) -> &TransmissionFields {
    world
        .labels()
        .iter()
        .find_map(|e| match e {
            Label::Transmission(t) => Some(t.fields()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no label"))
}

fn first_text(world: &World, id: MessageId) -> String {
    world
        .message(id)
        .and_then(|m| m.part_text(0).ok().map(|t| t.into_owned()))
        .unwrap_or_default()
}

#[test]
fn the_dataset_is_ct_evals() {
    assert_eq!(DATASET, "cipher");
    assert_eq!(VERSION, 1);
    assert_eq!(PAIRS_PER_CIPHER, 24);
    let defaults = Options::default();
    assert_eq!((defaults.count, defaults.seed), (24, 0));
    assert_eq!(
        defaults.settings(),
        BTreeMap::from([
            ("corpus_seed".to_owned(), Setting::Int(0)),
            ("count".to_owned(), Setting::Int(24)),
        ])
    );
    let big = Options {
        seed: u64::MAX,
        ..Options::default()
    };
    assert_eq!(
        big.settings().get("corpus_seed"),
        Some(&Setting::Text(u64::MAX.to_string()))
    );
}

#[test]
fn encoders_match_known_vectors() {
    assert_eq!(base64("hello"), "aGVsbG8=");
    assert_eq!(hex("Hi!"), "486921");
    assert_eq!(url("a b/c~d"), "a%20b%2Fc~d");
    assert_eq!(rot("Hello, World", 13), "Uryyb, Jbeyq");
    assert_eq!(rot("abc", 1), "bcd");
    assert_eq!(binary8("Hi"), "01001000 01101001");
    let mut key: [u8; 26] = std::array::from_fn(|at| b'a' + at as u8);
    key.reverse();
    assert_eq!(substitute("Abc z", &key), "Zyx a");
    assert_eq!(Cipher::Base64Url.encode("hello?"), "aGVsbG8%2F");
}

#[test]
fn needs_follow_the_actual_encoding() {
    let decoded = |codecs: Vec<Codec>| (MatchNeed::Decoded { codecs }, Tier::Construction);
    // URL encoding leaves letters and digits alone: nothing to decode.
    assert_eq!(
        Cipher::Url.need("plainletters42"),
        (MatchNeed::Exact, Tier::Construction)
    );
    assert_eq!(
        Cipher::Url.need("two words"),
        decoded(vec![Codec::UrlEncoding])
    );
    assert_eq!(Cipher::Base64.need("hello"), decoded(vec![Codec::Base64]));
    // base64("hello?") has a `/`, so URL encoding changes it: two layers,
    // listed in the order the reader's text is decoded.
    assert_eq!(
        Cipher::Base64Url.need("hello?"),
        decoded(vec![Codec::UrlEncoding, Codec::Base64])
    );
    // base64("abc") is `YWJj`: URL encoding is a no-op layer.
    assert_eq!(Cipher::Base64Url.need("abc"), decoded(vec![Codec::Base64]));
    assert_eq!(
        Cipher::Rot { shift: 13 }.need("letters"),
        (
            MatchNeed::Undecodable {
                codec: "rot13".into()
            },
            Tier::OutOfReach
        )
    );
    assert_eq!(
        Cipher::Rot { shift: 13 }.need("12345"),
        (MatchNeed::Exact, Tier::Construction)
    );
    assert_eq!(Cipher::Binary8.need("x").1, Tier::OutOfReach);
    for kind in CipherKind::ALL {
        let cipher = kind.instantiate(&mut SplitMix64::new(9));
        assert_eq!(cipher.kind(), kind);
        assert_eq!(
            kind.in_reach(),
            cipher.need("some letters").1 == Tier::Construction
        );
    }
}

#[test]
fn keys_are_drawn_from_the_seed() {
    let draw = |seed| CipherKind::RotN.instantiate(&mut SplitMix64::new(seed));
    for seed in 0..64 {
        let Cipher::Rot { shift } = draw(seed) else {
            panic!("rotN draws a shift");
        };
        assert!((1..=25).contains(&shift) && shift != 13, "{shift}");
    }
    assert_eq!(draw(5), draw(5));
    let Cipher::Substitution { key } =
        CipherKind::Substitution.instantiate(&mut SplitMix64::new(1))
    else {
        panic!("substitution draws a key");
    };
    let mut sorted = key;
    sorted.sort_unstable();
    assert_eq!(sorted, std::array::from_fn(|at| b'a' + at as u8));
}

#[test]
fn pools_are_loaded_by_name() {
    let pools = all_pools();
    let names: Vec<&str> = pools.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["long_sentences", "random_tokens", "short_phrases"]
    );
    // Blank lines are not payloads.
    assert_eq!(pools[2].payloads.len(), 3);
    let only = load(&root(), None, &["random".into()]).unwrap().0;
    assert_eq!(only.len(), 1);
    // Of the default pools, only short_phrases is in the fixture directory.
    assert!(DEFAULT_POOLS.contains(&"short_phrases"));
    let (defaults, files) = load(&root(), None, &[]).unwrap();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0].name, "short_phrases");
    assert_eq!(files, vec!["short_phrases.txt".to_owned()]);
    assert_eq!(load(&root(), Some(2), &["".into()]).unwrap().0.len(), 2);
    assert!(load(&root(), None, &["no_such_pool".into()]).is_err());
}

#[test]
fn a_pair_world_labels_the_encoded_bytes() {
    for (index, delivery) in [(0, Delivery::UserTurn), (1, Delivery::ToolResult)] {
        let pair = Pair::plan(CipherKind::Base64, index, &long_pool(), 3).unwrap();
        assert_eq!(pair.delivery, delivery);
        let built = world(&pair, Pace::DEFAULT).unwrap();
        assert_eq!(built.decl().agents.len(), 2);
        // A tool-result delivery takes the receiver two exchanges: the call,
        // then its result in the next request.
        let receiver = if delivery == Delivery::ToolResult {
            2
        } else {
            1
        };
        assert_eq!(built.exchanges().len(), 1 + receiver);
        assert_eq!(
            built.coverage(),
            Coverage::Complete {
                tier: Tier::Construction
            }
        );
        let label = label(&built);
        assert_eq!(label.content.text, base64(&pair.payload));
        assert_eq!(label.route, Route::Direct);
        assert_eq!(
            label.needs,
            MatchNeed::Decoded {
                codecs: vec![Codec::Base64]
            }
        );
        let reader = built.exchange(label.reader_exchange).unwrap();
        assert!(reader.request.messages.contains(&label.content.at.message));
        let at = label.content.at;
        let text = first_text(&built, at.message);
        assert_eq!(
            &text[at.range.start() as usize..at.range.end() as usize],
            label.content.text
        );
        let sender = label
            .sender_exchange
            .and_then(|id| built.exchange(id))
            .unwrap();
        assert!(sender.at_us < reader.at_us);
        assert_eq!(reader.at_us, compose(2, 0, 0).unwrap());
        let said = first_text(&built, sender.response.messages[0]);
        assert!(said.ends_with(&pair.payload));
        match delivery {
            Delivery::UserTurn => {
                assert_eq!(label.carrier, CarrierKind::UserTurn);
                assert_eq!(at.range.start() as usize, DELIVERY_HEADER.len());
            }
            Delivery::ToolResult => assert_eq!(label.carrier, CarrierKind::ToolResult),
        }
        assert_eq!(built.key().as_str(), format!("base64-00{index}"));
        for exchange in built.exchanges() {
            assert_eq!(exchange.client.model.as_deref(), Some("synthetic/cipher"));
            assert_eq!(exchange.source.file(), "cipher/long");
        }
    }
}

#[test]
fn out_of_reach_labels_need_an_undecodable_codec() {
    let pair = Pair::plan(CipherKind::Binary8, 0, &long_pool(), 0).unwrap();
    let built = world(&pair, Pace::DEFAULT).unwrap();
    let label = label(&built).clone();
    assert_eq!(label.tier, Tier::OutOfReach);
    assert_eq!(
        label.needs,
        MatchNeed::Undecodable {
            codec: "binary8".into()
        }
    );
    // The two go together, both ways.
    let mut reached = label.clone();
    reached.tier = Tier::Construction;
    assert_eq!(ExpectedTransmission::new(reached), Err(InvalidLabel::Reach));
    let mut decodable = label.clone();
    decodable.needs = MatchNeed::Exact;
    assert_eq!(
        ExpectedTransmission::new(decodable),
        Err(InvalidLabel::Reach)
    );
    // And a deserialized label is checked the same way.
    let mut json = serde_json::to_value(Label::Transmission(
        ExpectedTransmission::new(label).unwrap(),
    ))
    .unwrap();
    json["tier"] = serde_json::json!("construction");
    assert!(serde_json::from_value::<Label>(json).is_err());
}

#[test]
fn a_label_out_of_reach_is_built_like_any_other() {
    let reader = ExchangeId::from_raw(1);
    let made = ExpectedTransmission::new(TransmissionFields {
        id: LabelId::new("t").unwrap(),
        from: AgentKey::new("a").unwrap(),
        to: AgentKey::new("b").unwrap(),
        sender_exchange: None,
        reader_exchange: reader,
        route: Route::Direct,
        carrier: CarrierKind::UserTurn,
        content: ExpectedContent {
            text: "01100001".into(),
            at: Location {
                exchange: reader,
                message: a2a_bench_format::message::Message::new(
                    a2a_bench_format::message::Body::User(vec![
                        a2a_bench_format::message::UserPart::Text {
                            text: "Message:\n01100001".into(),
                        },
                    ]),
                )
                .unwrap()
                .id(),
                part: 0,
                range: ByteRange::new(9, 17).unwrap(),
            },
        },
        needs: MatchNeed::Undecodable {
            codec: "binary8".into(),
        },
        tier: Tier::OutOfReach,
        source: SourceRef::new("f", "/p"),
    });
    assert!(made.is_ok());
}

#[test]
fn the_source_plans_every_cipher_deterministically() {
    let source = |pairs, seed| CipherSource::new(all_pools(), pairs, seed).unwrap();
    let planned = source(4, 7).plan();
    assert_eq!(planned.len(), CipherKind::ALL.len() * 4);
    assert_eq!(planned, source(4, 7).plan());
    assert_ne!(planned, source(4, 8).plan());
    // Pools are used in turn.
    let pools: Vec<&str> = planned[..3].iter().map(|p| p.pool.as_str()).collect();
    assert_eq!(
        pools,
        vec!["long_sentences", "random_tokens", "short_phrases"]
    );
    let only = source(2, 7).with_kinds(vec![CipherKind::Hex]);
    assert_eq!(only.plan().len(), 2);
    assert_eq!(planned[0].world_key(), "base64-000");
}

/// ct-eval's reference-matcher test over these pairs counted 8 in-reach
/// and 8 out-of-reach labels (the matcher is another crate): the corpus
/// side of it.
#[test]
fn in_reach_and_out_of_reach_ciphers_are_labelled_apart() {
    let mut source = CipherSource::new(long_pool(), 2, 5).unwrap();
    let worlds: Vec<World> = source.worlds().map(|w| w.unwrap()).collect();
    assert_eq!(worlds.len(), 16);
    let mut tiers: BTreeMap<Tier, usize> = BTreeMap::new();
    for world in &worlds {
        let label = label(world);
        *tiers.entry(label.tier).or_default() += 1;
        let kind = world.key().as_str().rsplit_once('-').unwrap().0;
        let in_reach = ["base64", "hex", "url", "base64_url"].contains(&kind);
        assert_eq!(label.tier == Tier::Construction, in_reach, "{kind}");
        // Nothing else is labelled: coverage is complete.
        let others = world
            .labels()
            .iter()
            .filter(|l| !matches!(l, Label::ExchangeAgent(_) | Label::Transmission(_)))
            .count();
        assert_eq!(others, 0);
    }
    assert_eq!(tiers.get(&Tier::Construction), Some(&8));
    assert_eq!(tiers.get(&Tier::OutOfReach), Some(&8));
}

#[test]
fn the_source_reads_its_pools_and_paces_its_calls() {
    let pace = Pace::new(
        std::time::Duration::from_millis(1_000),
        std::time::Duration::from_millis(5_000),
        9,
    )
    .unwrap();
    let options = Options {
        count: 2,
        seed: 9,
        ..Options::default()
    };
    let mut source = source(&root(), &options, pace).unwrap();
    assert_eq!(source.files_read().len(), 1);
    let worlds: Vec<World> = source.worlds().map(|w| w.unwrap()).collect();
    assert_eq!(worlds.len(), 16);
    let times: Vec<Timestamp> = worlds[1].exchanges().iter().map(|e| e.at_us).collect();
    assert_eq!(
        times,
        vec![
            pace.at(0, 0, 0).unwrap(),
            pace.at(1, 0, 0).unwrap(),
            pace.at(2, 0, 0).unwrap()
        ]
    );
}
