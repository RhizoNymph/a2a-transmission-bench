//! Percent-encoding: canonical form (RFC 3986 §6.2.2) and decoding.

/// `text` with percent-encoding in canonical form: an escaped unreserved
/// character is decoded, every other escape keeps its byte with upper-case
/// hex. A `%` not followed by two hex digits is kept as it is.
pub(crate) fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        match escape(rest.as_bytes()) {
            Some(byte) if is_unreserved(byte) => {
                out.push(char::from(byte));
                rest = rest.get(3..).unwrap_or_default();
            }
            Some(byte) => {
                out.push_str(&format!("%{byte:02X}"));
                rest = rest.get(3..).unwrap_or_default();
            }
            None => {
                out.push(character);
                rest = rest.get(character.len_utf8()..).unwrap_or_default();
            }
        }
    }
    out
}

/// `text` with every `%XX` escape decoded; text whose decoded bytes are not
/// UTF-8 is kept as it was.
pub(crate) fn decode(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let [first, tail @ ..] = rest {
        match escape(rest) {
            Some(byte) => {
                out.push(byte);
                rest = rest.get(3..).unwrap_or_default();
            }
            None => {
                out.push(*first);
                rest = tail;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// A URL path's non-empty segments, each decoded.
pub(crate) fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(decode)
        .collect()
}

/// The byte `%XX` at the start of `bytes` escapes.
fn escape(bytes: &[u8]) -> Option<u8> {
    match bytes {
        [b'%', high, low, ..] => Some(hex_value(*high)? * 16 + hex_value(*low)?),
        _ => None,
    }
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_decodes_unreserved_and_uppercases_the_rest() {
        assert_eq!(normalize("/%7euser/%2fx/%41"), "/~user/%2Fx/A");
        assert_eq!(normalize("/%zz/%4/ü%"), "/%zz/%4/ü%");
    }

    #[test]
    fn decode_keeps_text_that_does_not_decode_to_utf8() {
        assert_eq!(decode("a%2Fb%20c"), "a/b c");
        assert_eq!(decode("%FF%FE"), "%FF%FE");
        assert_eq!(decode("%e2%82%ac%"), "€%");
    }

    #[test]
    fn segments_skip_empty_ones() {
        assert_eq!(segments("//o/%6E//"), vec!["o", "n"]);
    }
}
