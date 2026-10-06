//! The match a delivered SALT message needs (crosstalk-eval's
//! `MatchNeed::through_json_string`, which the format crate does not carry).

use a2a_bench_format::labels::{Codec, MatchNeed};

/// Whether writing `text` as a JSON string changes it: it holds a quote, a
/// backslash or a control character.
pub fn json_escapes(text: &str) -> bool {
    text.chars()
        .any(|ch| matches!(ch, '"' | '\\' | '\u{0}'..='\u{1f}'))
}

/// Text a writer put inside a JSON string (tool-call arguments) and a
/// reader received raw: `decoded [json_string]` when writing it as a JSON
/// string changes it, `exact` otherwise. Undoing escapes is decoding, never
/// normalization.
pub fn through_json_string(text: &str) -> MatchNeed {
    if json_escapes(text) {
        MatchNeed::Decoded {
            codecs: vec![Codec::JsonString],
        }
    } else {
        MatchNeed::Exact
    }
}
