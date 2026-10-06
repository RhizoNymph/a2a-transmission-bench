//! Candidate encoded tokens of a payload: base64, hex and URL-encoded runs,
//! the inside of `atob('...')`, and `\x..` escape runs, in that order (a
//! token's index in this list names its world).

/// Candidate encoded tokens of `text`, 16 bytes or longer, duplicates kept
/// (the caller keeps a token's first index).
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    // atob('…') / atob("…") inner strings.
    let mut rest = text;
    while let Some(at) = rest.find("atob(") {
        let after = &rest[at + 5..];
        if let Some(inner) = quoted(after) {
            out.push(inner);
        }
        // Past the first character after `atob(` (crosstalk-eval stepped one
        // byte, which panics where that character is not ASCII).
        let step = after.chars().next().map_or(0, char::len_utf8);
        rest = after.get(step..).unwrap_or("");
    }
    out.extend(runs(text, |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_')
    }));
    out.extend(runs(text, |b| b.is_ascii_hexdigit()));
    out.extend(runs(text, |b| !b.is_ascii_whitespace() && b != b'"'));
    out.extend(escape_runs(text));
    out.retain(|token| token.len() >= 16);
    out
}

/// The contents of the first single- or double-quoted string in `text`.
fn quoted(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let quote = *bytes.first()?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let end = text[1..].find(char::from(quote))?;
    Some(text[1..1 + end].to_owned())
}

/// Maximal runs of bytes satisfying `keep`, as owned strings.
fn runs(text: &str, keep: impl Fn(u8) -> bool) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = None;
    for (at, &byte) in bytes.iter().enumerate() {
        let inside = byte.is_ascii() && keep(byte);
        match (inside, start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                out.push(text[from..at].to_owned());
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        out.push(text[from..].to_owned());
    }
    out
}

/// Maximal runs made of `\xNN` escapes.
fn escape_runs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        if &bytes[at..at + 2] == b"\\x"
            && bytes[at + 2].is_ascii_hexdigit()
            && bytes[at + 3].is_ascii_hexdigit()
        {
            let start = at;
            while at + 4 <= bytes.len()
                && &bytes[at..at + 2] == b"\\x"
                && bytes[at + 2].is_ascii_hexdigit()
                && bytes[at + 3].is_ascii_hexdigit()
            {
                at += 4;
            }
            out.push(text[start..at].to_owned());
        } else {
            at += 1;
        }
    }
    out
}
