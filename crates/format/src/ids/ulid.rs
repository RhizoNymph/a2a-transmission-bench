//! 128-bit ids as 26-character Crockford base32 text, as ULIDs are written.

use std::fmt;

/// Crockford's alphabet: digits and upper-case letters without I, L, O and U.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The length of a ULID's text.
pub const ULID_LEN: usize = 26;

/// 128 bits as 26 base32 digits: the first carries the top 3 bits, each
/// later one 5.
pub fn encode(raw: u128) -> String {
    (0..26u32)
        .rev()
        .map(|digit| {
            let index = (raw >> (5 * digit)) & 0x1f;
            // index < 32 by the mask.
            char::from(CROCKFORD[usize::try_from(index).unwrap_or(0)])
        })
        .collect()
}

/// Why text is not a ULID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidUlid {
    #[error("a ULID is 26 characters, got {got}")]
    Length { got: usize },
    /// Lower case and the excluded letters are refused, so every id has one text.
    #[error("character {index} is not upper-case Crockford base32")]
    Character { index: usize },
    #[error("the first character is above 7: more than 128 bits")]
    Overflow,
}

/// The bits `text` spells.
pub fn decode(text: &str) -> Result<u128, InvalidUlid> {
    let bytes = text.as_bytes();
    if bytes.len() != ULID_LEN {
        return Err(InvalidUlid::Length { got: bytes.len() });
    }
    let mut raw: u128 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        let value = CROCKFORD
            .iter()
            .position(|c| c == byte)
            .ok_or(InvalidUlid::Character { index })?;
        if index == 0 && value > 7 {
            return Err(InvalidUlid::Overflow);
        }
        raw = (raw << 5) | u128::try_from(value).map_err(|_| InvalidUlid::Character { index })?;
    }
    Ok(raw)
}

/// A ULID-shaped id, displayed as its text.
pub(crate) struct UlidText(pub(crate) u128);

impl fmt::Display for UlidText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode(self.0))
    }
}
