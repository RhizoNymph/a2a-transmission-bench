//! The reference matcher's matching fold, as crosstalk-eval's
//! `reference/fold.rs` computes its text (the raw-range bookkeeping, which
//! the forwarding rule does not use, is left out). In one pass over the raw
//! text:
//!
//! 1. **String escapes are unfolded**, whatever their nesting depth: a run
//!    of backslashes before `n`, `t`, `r`, `b` or `f` becomes whitespace,
//!    before `uXXXX` the character it names, before a line break nothing (a
//!    YAML line continuation, which also drops the next line's
//!    indentation), and before anything else is dropped (`\"` is `"`, `\\`
//!    is `\`). Trailing backslashes end the text.
//! 2. **Case is folded** (`char::to_lowercase`).
//! 3. **Whitespace is collapsed**: every run becomes one space; leading
//!    whitespace is dropped.

struct Writer {
    text: String,
    /// The last thing written was whitespace (or nothing yet).
    in_space: bool,
}

impl Writer {
    fn push(&mut self, ch: char) {
        if ch.is_whitespace() {
            if !self.in_space {
                self.text.push(' ');
                self.in_space = true;
            }
            return;
        }
        self.in_space = false;
        self.text.extend(ch.to_lowercase());
    }
}

/// `raw` folded.
pub fn fold(raw: &str) -> String {
    let mut writer = Writer {
        text: String::with_capacity(raw.len()),
        in_space: true,
    };
    let chars: Vec<char> = raw.chars().collect();
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        if ch != '\\' {
            writer.push(ch);
            at += 1;
            continue;
        }
        let mut next = at;
        while chars.get(next) == Some(&'\\') {
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
                // Not an escape: the backslashes go, the `u` stays.
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
                // `\"`, `\\` collapsed into the run, `\/`, anything else: the
                // backslashes go, the character stays.
                writer.push(escaped);
                at = next + 1;
            }
        }
    }
    writer.text
}

/// The character a `uXXXX` escape (with `XXXX` at `at`) names, and the index
/// after it; a surrogate pair written as two escapes is combined, and a
/// lone high surrogate is U+FFFD.
fn unicode_escape(chars: &[char], at: usize) -> Option<(char, usize)> {
    let high = hex4(chars, at)?;
    if (0xD800..0xDC00).contains(&high) {
        // Expect `\uDCxx` next (any number of backslashes).
        let mut next = at + 4;
        let mut slashes = 0;
        while chars.get(next) == Some(&'\\') {
            next += 1;
            slashes += 1;
        }
        if slashes > 0 && chars.get(next) == Some(&'u') {
            let low = hex4(chars, next + 1)?;
            if (0xDC00..0xE000).contains(&low) {
                let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return char::from_u32(combined).map(|ch| (ch, next + 5));
            }
        }
        return Some(('\u{FFFD}', at + 4));
    }
    char::from_u32(high).map(|ch| (ch, at + 4))
}

fn hex4(chars: &[char], at: usize) -> Option<u32> {
    chars
        .get(at..at + 4)?
        .iter()
        .try_fold(0u32, |value, c| Some(value * 16 + c.to_digit(16)?))
}
