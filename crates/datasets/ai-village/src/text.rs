//! Locating labelled text and deciding the match it needs.

use a2a_bench_format::labels::{Codec, MatchNeed};
use a2a_bench_format::message::{AssistantPart, Body, Message};

use super::fold::{fold, fold_plain, unescape_once};

/// The `Undecodable` codec name of text escaped two string levels deep.
pub const TWO_STRING_LEVELS: &str = "json_string+json_string";

/// Text serialised once as a JSON string: `decoded [json_string]`.
pub fn json_string() -> MatchNeed {
    MatchNeed::Decoded {
        codecs: vec![Codec::JsonString],
    }
}

/// Text that arrives only after two string levels are undone: out of
/// reach, since a decoded chain holds at most one string codec.
pub fn two_string_levels() -> MatchNeed {
    MatchNeed::Undecodable {
        codec: TWO_STRING_LEVELS.to_owned(),
    }
}

/// `text` as the contents of a JSON string (no quotes), escaped the way
/// both `JSON.stringify` and serde_json escape: `"`, `\`, `\b`, `\f`, `\n`,
/// `\r`, `\t`, and other control characters as `\u00xx`.
pub fn json_escape(text: &str) -> String {
    let quoted = serde_json::Value::String(text.to_owned()).to_string();
    quoted
        .get(1..quoted.len().saturating_sub(1))
        .unwrap_or_default()
        .to_owned()
}

/// The value of `inner` read as the inside of a JSON string literal; `None`
/// when it is not one (a bare `"`, a raw control character, a bad escape).
pub fn json_unescape(inner: &str) -> Option<String> {
    serde_json::from_str(&format!("\"{inner}\"")).ok()
}

/// The byte range of `needle` in `haystack`: the last occurrence starting
/// before `before` when given and there is one, else the first.
pub fn find(haystack: &str, needle: &str, before: Option<usize>) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let start = match before {
        Some(limit) => haystack
            .get(..limit.min(haystack.len()))
            .and_then(|head| head.rfind(needle))
            .or_else(|| haystack.find(needle)),
        None => haystack.find(needle),
    }?;
    Some((start, start + needle.len()))
}

/// The texts of every part of `message` that has one.
pub fn part_texts(message: &Message) -> Vec<String> {
    (0..message.part_count())
        .filter_map(|part| u16::try_from(part).ok())
        .filter_map(|part| message.part_text(part).ok().map(|t| t.into_owned()))
        .collect()
}

/// The weakest match a detector needs to tie `read` (the reader's bytes) to
/// the sender's `response`:
///
/// - `exact` when the bytes occur verbatim in one of its parts;
/// - `decoded [json_string]` when `read` is the inside of a JSON string
///   whose value (one level of unescaping) occurs verbatim;
/// - `normalized` when they occur after case and whitespace folding alone;
/// - `decoded [json_string]` when they do once one string level is undone
///   on either side, then folded;
/// - out of reach ([`two_string_levels`]) when undoing exactly two string
///   levels on one side makes them equal;
/// - `decoded [json_string]` when only the matching fold (escapes undone
///   at any depth) makes them equal, with no one- or two-level reading;
/// - else `semantic`.
pub fn need(response: &Message, read: &str) -> MatchNeed {
    let texts = part_texts(response);
    if texts.iter().any(|text| text.contains(read)) {
        return MatchNeed::Exact;
    }
    if let Some(value) = json_unescape(read)
        && value != read
        && !value.trim().is_empty()
        && texts.iter().any(|text| text.contains(&value))
    {
        return json_string();
    }
    let plain = fold_plain(read);
    let plain = plain.trim_end();
    if plain.is_empty() {
        return MatchNeed::Semantic;
    }
    let plains: Vec<String> = texts.iter().map(|text| fold_plain(text)).collect();
    if plains.iter().any(|text| text.contains(plain)) {
        return MatchNeed::Normalized;
    }
    let read_once = fold_plain(&unescape_once(read));
    let read_once = read_once.trim_end();
    let unescaped: Vec<String> = texts
        .iter()
        .map(|text| fold_plain(&unescape_once(text)))
        .collect();
    let one_level = |needle: &str| {
        !needle.is_empty()
            && plains
                .iter()
                .chain(&unescaped)
                .any(|text| text.contains(needle))
    };
    if one_level(read_once) || unescaped.iter().any(|text| text.contains(plain)) {
        return json_string();
    }
    let read_twice = fold_plain(&unescape_once(&unescape_once(read)));
    let read_twice = read_twice.trim_end();
    let two_levels = (!read_twice.is_empty()
        && plains.iter().any(|text| text.contains(read_twice)))
        || texts
            .iter()
            .any(|text| fold_plain(&unescape_once(&unescape_once(text))).contains(plain));
    if two_levels {
        return two_string_levels();
    }
    // Equal under the matching fold only, with no one- or two-level
    // reading of the whole text: classed as one string level.
    let folded = fold(read);
    if !folded.trim().is_empty() && texts.iter().any(|text| fold(text).contains(&folded)) {
        return json_string();
    }
    MatchNeed::Semantic
}

/// The text of an assistant body's text and reasoning parts, joined, for
/// keyword checks.
pub fn visible_text(body: &Body) -> String {
    match body {
        Body::Assistant(parts) => parts
            .iter()
            .filter_map(|part| match part {
                AssistantPart::Text { text } | AssistantPart::Reasoning { text } => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The tier a label with `need` gets: `out_of_reach` when the need is,
/// `in_reach` otherwise.
pub fn tier(
    need: &MatchNeed,
    in_reach: a2a_bench_format::labels::Tier,
) -> a2a_bench_format::labels::Tier {
    if need.out_of_reach() {
        a2a_bench_format::labels::Tier::OutOfReach
    } else {
        in_reach
    }
}

/// The class of a need, as ct-eval's stats count it (`exact`,
/// `normalized`, `decoded`, `semantic`).
pub fn class_name(need: &MatchNeed) -> &'static str {
    use a2a_bench_format::labels::MatchClass;
    let class = match need {
        MatchNeed::Exact => MatchClass::Exact,
        MatchNeed::Normalized => MatchClass::Normalized,
        MatchNeed::Decoded { .. } | MatchNeed::Undecodable { .. } => MatchClass::Decoded,
        MatchNeed::Semantic => MatchClass::Semantic,
        MatchNeed::Unobserved { arrival, .. } => *arrival,
    };
    match class {
        MatchClass::Exact => "exact",
        MatchClass::Normalized => "normalized",
        MatchClass::Decoded => "decoded",
        MatchClass::Semantic => "semantic",
    }
}
