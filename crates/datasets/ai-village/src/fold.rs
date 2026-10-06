//! The text folds a label's match need is decided under (ct-eval's
//! reference matcher's folds at 7f8a2fb, text only).
//!
//! - [`fold_plain`]: case and whitespace folding (`char::to_lowercase`;
//!   every whitespace run one space; leading whitespace dropped): what
//!   `normalized` compares under.
//! - [`unescape_once`]: one level of string escapes undone (JSON's, or a
//!   YAML double-quoted scalar's), leniently on a cut of a literal.
//! - [`fold`]: the matching fold: string escapes unfolded at any depth,
//!   then case and whitespace folded.

struct Writer {
    text: String,
    /// The last thing written was whitespace (or nothing yet).
    in_space: bool,
}

impl Writer {
    fn new(capacity: usize) -> Self {
        Self {
            text: String::with_capacity(capacity),
            in_space: true,
        }
    }

    fn push(&mut self, ch: char) {
        if ch.is_whitespace() {
            if !self.in_space {
                self.text.push(' ');
                self.in_space = true;
            }
            return;
        }
        self.in_space = false;
        for lower in ch.to_lowercase() {
            self.text.push(lower);
        }
    }
}

/// Case and whitespace folding only, escapes left as they are.
pub fn fold_plain(raw: &str) -> String {
    let mut writer = Writer::new(raw.len());
    for ch in raw.chars() {
        writer.push(ch);
    }
    writer.text
}

/// `raw` with exactly one level of string escapes undone: each backslash
/// escape (JSON's, or a YAML double-quoted scalar's) becomes what it
/// names, so `\\n` becomes `\n` (a backslash and an `n`), never a line
/// break. An escaped line break drops the break and the next line's
/// indentation. An escape that names nothing (`\q`, a short `\u`) and a
/// trailing backslash are kept as written.
pub fn unescape_once(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut at = 0;
    while at < chars.len() {
        let ch = chars[at];
        if ch != '\\' {
            out.push(ch);
            at += 1;
            continue;
        }
        let Some(&escaped) = chars.get(at + 1) else {
            out.push(ch);
            break;
        };
        let simple = match escaped {
            'n' => Some('\n'),
            't' | '\t' => Some('\t'),
            'r' => Some('\r'),
            'b' => Some('\u{8}'),
            'f' => Some('\u{c}'),
            '0' => Some('\0'),
            'a' => Some('\u{7}'),
            'e' => Some('\u{1b}'),
            'v' => Some('\u{b}'),
            'N' => Some('\u{85}'),
            '_' => Some('\u{a0}'),
            'L' => Some('\u{2028}'),
            'P' => Some('\u{2029}'),
            '"' | '\\' | '/' | ' ' | '\'' => Some(escaped),
            _ => None,
        };
        if let Some(decoded) = simple {
            out.push(decoded);
            at += 2;
            continue;
        }
        match escaped {
            '\n' | '\r' => {
                let mut after = at + 2;
                if escaped == '\r' && chars.get(after) == Some(&'\n') {
                    after += 1;
                }
                while matches!(chars.get(after), Some(' ' | '\t')) {
                    after += 1;
                }
                at = after;
            }
            'u' => match utf16_escape(&chars, at + 2) {
                Some((decoded, after)) => {
                    out.push(decoded);
                    at = after;
                }
                None => {
                    out.push(ch);
                    at += 1;
                }
            },
            'x' | 'U' => {
                let digits = if escaped == 'x' { 2 } else { 8 };
                match hex_n(&chars, at + 2, digits).and_then(char::from_u32) {
                    Some(decoded) => {
                        out.push(decoded);
                        at += 2 + digits;
                    }
                    None => {
                        out.push(ch);
                        at += 1;
                    }
                }
            }
            _ => {
                out.push(ch);
                at += 1;
            }
        }
    }
    out
}

/// A `uXXXX` escape's character (with `XXXX` at `at`), a surrogate pair
/// written as two single-backslash escapes combined, and the index after
/// it.
fn utf16_escape(chars: &[char], at: usize) -> Option<(char, usize)> {
    let high = hex_n(chars, at, 4)?;
    if (0xD800..0xDC00).contains(&high) {
        if chars.get(at + 4) == Some(&'\\') && chars.get(at + 5) == Some(&'u') {
            let low = hex_n(chars, at + 6, 4)?;
            if (0xDC00..0xE000).contains(&low) {
                let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return char::from_u32(combined).map(|ch| (ch, at + 10));
            }
        }
        return None;
    }
    char::from_u32(high).map(|ch| (ch, at + 4))
}

fn hex_n(chars: &[char], at: usize, n: usize) -> Option<u32> {
    chars
        .get(at..at + n)?
        .iter()
        .try_fold(0u32, |value, c| Some(value * 16 + c.to_digit(16)?))
}

/// The matching fold of `raw`: string escapes unfolded whatever their
/// nesting depth (a run of backslashes before `n`, `t`, `r`, `b` or `f`
/// is whitespace, before `uXXXX` the character it names, before a line
/// break nothing, before anything else dropped), then case and whitespace
/// folded.
pub fn fold(raw: &str) -> String {
    let mut writer = Writer::new(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let ch = chars[at];
        if ch != '\\' {
            writer.push(ch);
            at += 1;
            continue;
        }
        let mut next = at;
        while next < chars.len() && chars[next] == '\\' {
            next += 1;
        }
        let Some(&escaped) = chars.get(next) else {
            // Trailing backslashes: nothing to unfold.
            break;
        };
        match escaped {
            'n' | 't' | 'r' | 'b' | 'f' => {
                writer.push(' ');
                at = next + 1;
            }
            'u' => match unicode_escape(&chars, next + 1) {
                Some((decoded, after)) => {
                    writer.push(decoded);
                    at = after;
                }
                None => at = next,
            },
            '\n' | '\r' => {
                // YAML line continuation: drop the break and the indentation.
                let mut after = next + 1;
                if escaped == '\r' && chars.get(after) == Some(&'\n') {
                    after += 1;
                }
                while matches!(chars.get(after), Some(' ' | '\t')) {
                    after += 1;
                }
                at = after;
            }
            _ => {
                writer.push(escaped);
                at = next + 1;
            }
        }
    }
    writer.text
}

/// The character a `uXXXX` escape (with `XXXX` at `at`) names, and the
/// index after it; a surrogate pair written as two escapes is combined.
fn unicode_escape(chars: &[char], at: usize) -> Option<(char, usize)> {
    let high = hex_n(chars, at, 4)?;
    if (0xD800..0xDC00).contains(&high) {
        let mut next = at + 4;
        let mut slashes = 0;
        while chars.get(next) == Some(&'\\') {
            next += 1;
            slashes += 1;
        }
        if slashes > 0 && chars.get(next) == Some(&'u') {
            let low = hex_n(chars, next + 1, 4)?;
            if (0xDC00..0xE000).contains(&low) {
                let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return char::from_u32(combined).map(|ch| (ch, next + 5));
            }
        }
        return Some(('\u{FFFD}', at + 4));
    }
    char::from_u32(high).map(|ch| (ch, at + 4))
}
