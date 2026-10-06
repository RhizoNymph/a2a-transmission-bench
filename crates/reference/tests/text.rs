//! Folding, classification, decoding, shingles, opaque blobs and routes
//! (ported from crosstalk-eval's `tests/reference.rs`, on bench types).
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::labels::Codec;
use a2a_bench_format::message::{ToolArguments, ToolCall, ToolExecution};
use a2a_bench_format::predictions::MatchKind;
use a2a_bench_format::resource::Resource;
use a2a_bench_reference::route::{
    ChannelResource, Url, extract_resource, normalize_path, parse_url,
};
use a2a_bench_reference::text::classify::Classified::{self, Match};
use a2a_bench_reference::text::classify::classify;
use a2a_bench_reference::text::decode::decode_candidates;
use a2a_bench_reference::text::fold::{fold, fold_plain, string_codec, unescape_once};
use a2a_bench_reference::text::opaque::{opaque_ranges, segments};
use a2a_bench_reference::text::shingle::{covered, shingles};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const SENTENCE: &str = "The vendor table has eleven overdue approvals in March";

fn decoded(codec: Codec) -> Classified {
    Match(MatchKind::Decoded {
        codecs: vec![codec],
    })
}

#[test]
fn hits_are_classed_by_the_weakest_transformation() {
    // A span as it sits inside JSON tool-call arguments: escaped.
    let span = r#"Say \"hello\" to\nthe Vendor Desk today"#;
    let plain = fold_plain(span);
    assert_eq!(
        classify(span, &plain, r#"\"hello\" to"#),
        Match(MatchKind::Exact)
    );
    assert_eq!(
        classify(span, &plain, "THE   vendor desk"),
        Match(MatchKind::Normalized),
        "case and whitespace only"
    );
    assert_eq!(
        classify(span, &plain, "\"hello\" to\nthe vendor"),
        decoded(Codec::JsonString),
        "delivered unescaped: one level of JSON string decoding"
    );
    let yaml = "a long note that \\\n    continues on the next line";
    assert_eq!(
        classify(
            yaml,
            &fold_plain(yaml),
            "a long note that continues on the next line"
        ),
        decoded(Codec::YamlString),
        "an escaped line break is YAML's"
    );
}

#[test]
fn string_codecs_are_told_apart_by_their_escapes() {
    assert_eq!(string_codec(r#"a \"quote\" and é\n"#), Codec::JsonString);
    assert_eq!(string_codec(r"a \\ backslash then x"), Codec::JsonString);
    assert_eq!(string_codec(r"bell \a and \x41"), Codec::YamlString);
    assert_eq!(string_codec("continued \\\n here"), Codec::YamlString);
    assert_eq!(fold_plain("  Mixed\tCASE  text "), "mixed case text ");
}

#[test]
fn opaque_ranges_cover_ids_and_signature_values() {
    let text = r#"{"id": "call_1__thought__QUJD+/=", "signature": "c2lnbmF0dXJl\"x", "thought_signatures": ["a", "b"], "keep": "this"}"#;
    let ranges = opaque_ranges(text);
    let cut: Vec<&str> = ranges.iter().map(|&(s, e)| &text[s..e]).collect();
    assert_eq!(
        cut,
        vec![
            "call_1__thought__QUJD+/=",
            "\"c2lnbmF0dXJl\\\"x\"",
            "[\"a\", \"b\"]"
        ]
    );
    let kept: String = segments(text).into_iter().map(|(_, piece)| piece).collect();
    assert!(kept.contains("\"keep\": \"this\""));
    assert!(!kept.contains("__thought__"));
    let escaped = r#"[{\"thought_signature\": \"QUJDREVG\", \"tool\": \"x\"}]"#;
    let cut: Vec<&str> = opaque_ranges(escaped)
        .iter()
        .map(|&(s, e)| &escaped[s..e])
        .collect();
    assert_eq!(cut, vec![r#""QUJDREVG\""#]);
}

#[test]
fn opaque_members_cover_every_listed_name() {
    for key in [
        "thought_signature",
        "thought_signatures",
        "signature",
        "encrypted_content",
        "redacted_thinking",
    ] {
        let text = format!(r#"{{"{key}": "QUJDREVGR0hJSktMTU5PUA==", "keep": "x"}}"#);
        let cut: Vec<&str> = opaque_ranges(&text)
            .iter()
            .map(|&(s, e)| &text[s..e])
            .collect();
        assert_eq!(cut, vec!["\"QUJDREVGR0hJSktMTU5PUA==\""], "{key}");
    }
    // A name that is not a member name (no quote before it) is kept.
    assert!(opaque_ranges("the signature: \"abc\"").is_empty());
}

#[test]
fn fold_unescapes_folds_case_and_collapses_whitespace() {
    let folded = fold("A\\\\\\\"B\\nC  \t D\\u00e9\\ud83d\\ude00 x\\\n   y", 0);
    assert_eq!(folded.text, "a\"b c d\u{e9}\u{1F600} xy");
    assert_eq!(folded.raw_range(0, 1), Some((0, 1)));
    let raw = "Hé  WORLD";
    let folded = fold(raw, 10);
    assert_eq!(folded.text, "hé world");
    assert_eq!(folded.raw_range(4, 9), Some((15, 20)));
}

#[test]
fn shingles_hash_every_window_and_cover_runs() {
    let windows = shingles(b"abcdabcd", 4);
    assert_eq!(windows.len(), 5);
    assert_eq!(windows[0].0, windows[4].0);
    assert_ne!(windows[0].0, windows[1].0);
    assert!(shingles(b"abc", 4).is_empty());
    assert_eq!(covered(&[0, 1, 9], 4), vec![(0, 5), (9, 13)]);
}

#[test]
fn decoding_drops_noise() {
    assert!(decode_candidates("plain words only, nothing encoded here at all", 0, 16).is_empty());
    assert!(decode_candidates("verificationprocessing", 0, 16).is_empty());
    let found = decode_candidates(&format!("x {} y", STANDARD.encode(SENTENCE)), 5, 16);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, SENTENCE);
    assert_eq!(found[0].start, 7);
    assert_eq!(found[0].codec, Codec::Base64);
}

#[test]
fn urls_and_paths_normalize() {
    let url = parse_url("HTTPS://Example.COM:443/a/b?z=1&a=2#frag");
    assert_eq!(
        url,
        Some(Url {
            scheme: "https".into(),
            host: "example.com".into(),
            path: "/a/b".into(),
            query: Some("a=2&z=1".into()),
        })
    );
    assert_eq!(
        url.map(|url| url.key()),
        Some("https://example.com/a/b?a=2&z=1".to_owned())
    );
    assert_eq!(
        parse_url("http://Host:80").map(|url| url.key()),
        Some("http://host/".to_owned())
    );
    assert_eq!(parse_url("ftp://x/y"), None);
    assert_eq!(parse_url("https://a b/c"), None);
    assert_eq!(normalize_path("/a//b/./c/../d"), "/a/b/d");
}

fn call(arguments: &str) -> ToolCall {
    ToolCall {
        call_id: "c".into(),
        name: "tool".into(),
        arguments: match CanonicalJson::canonicalize(arguments) {
            Ok(json) => ToolArguments::Json(json),
            Err(_) => ToolArguments::Invalid(arguments.into()),
        },
        execution: ToolExecution::Client,
    }
}

#[test]
fn resources_come_from_urls_and_absolute_paths() {
    let url = extract_resource(&call(r#"{"url": "https://x.example/p", "method": "GET"}"#));
    assert_eq!(
        url.as_ref().map(ChannelResource::resource),
        Some(Resource::Url("https://x.example/p".into()))
    );
    let file = extract_resource(&call(r#"{"file_path": "/srv//data/./a.txt"}"#));
    assert_eq!(
        file.as_ref().map(ChannelResource::resource),
        Some(Resource::File {
            path: "/srv/data/a.txt".into()
        })
    );
    assert_eq!(
        file.as_ref().map(ChannelResource::key),
        Some("file:///srv/data/a.txt".to_owned())
    );
    // A relative path is no canonical resource; neither is an unlisted key.
    assert_eq!(extract_resource(&call(r#"{"path": "notes.md"}"#)), None);
    assert_eq!(extract_resource(&call(r#"{"query": "/etc/passwd"}"#)), None);
    // Invalid arguments and non-objects name nothing.
    assert_eq!(extract_resource(&call("not json at all")), None);
    assert_eq!(extract_resource(&call(r#"["https://x.example/p"]"#)), None);
    // Members are sorted, so the first URL by member name wins.
    let both = extract_resource(&call(
        r#"{"z": "https://z.example/", "a": "https://a.example/"}"#,
    ));
    assert_eq!(both.map(|r| r.key()), Some("https://a.example/".to_owned()));
}

#[test]
fn unescape_once_undoes_exactly_one_string_level() {
    assert_eq!(
        unescape_once(r#"say \"hi\"\nthen go"#),
        "say \"hi\"\nthen go"
    );
    // An escaped backslash before `n` is a backslash and an `n`, never a
    // line break: one level only.
    assert_eq!(unescape_once(r"a \\n b"), r"a \n b");
    assert_eq!(unescape_once(r#"\\\"quoted\\\""#), r#"\"quoted\""#);
    assert_eq!(unescape_once(r"café 😀"), "café 😀");
    assert_eq!(unescape_once("long \\\n    line"), "long line");
    assert_eq!(
        unescape_once(r"bell \x41 and \q and end\"),
        r"bell A and \q and end\"
    );
}

#[test]
fn two_string_levels_are_out_of_reach() {
    // The span holds the text raw; the read holds it escaped twice.
    let span = r#"She said "move the meeting" and "bring the ledger" today"#;
    let plain = fold_plain(span);
    // One JSON string level: the contents of the literal, no quotes.
    let escape = |text: &str| {
        let quoted = serde_json::to_string(text).unwrap_or_default();
        quoted[1..quoted.len() - 1].to_owned()
    };
    let once = escape(span);
    let twice = escape(&once);
    assert_eq!(
        classify(span, &plain, &once),
        decoded(Codec::JsonString),
        "one level: in reach"
    );
    assert_eq!(
        classify(span, &plain, &twice),
        Classified::TwoStringLevels,
        "two levels: no spec decoder undoes them"
    );
    // A span written inside JSON arguments (escaped once) read escaped
    // twice is one level apart: in reach.
    assert_eq!(
        classify(&once, &fold_plain(&once), &twice),
        decoded(Codec::JsonString)
    );
}

#[test]
fn yaml_single_quotes_and_bridged_ranges_stay_in_reach() {
    let span = "use the subject 'All messages with Travel Agency' and the body";
    let plain = fold_plain(span);
    // A YAML single-quoted scalar doubles the quote: one YAML level.
    assert_eq!(
        classify(
            span,
            &plain,
            "use the subject ''All\n  messages with Travel Agency'' and the body"
        ),
        decoded(Codec::YamlString)
    );
    // A range the fold joined across one character neither side shares is
    // not two string levels: no reading explains it, so it stays in reach.
    let bridged = classify(
        span,
        &plain,
        "use the subject 'All messages with Travel Agency'X and the body",
    );
    assert_ne!(bridged, Classified::TwoStringLevels);
}
