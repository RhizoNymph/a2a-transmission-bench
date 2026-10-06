//! Locations: a byte range of one part of one message in one exchange.

use serde::{Deserialize, Deserializer, Serialize};

use crate::ids::{ExchangeId, MessageId};

/// A non-empty half-open byte range, `start < end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ByteRange {
    start: u32,
    end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("byte range {start}..{end} is empty or reversed")]
pub struct EmptyRange {
    pub start: u32,
    pub end: u32,
}

impl ByteRange {
    pub fn new(start: u32, end: u32) -> Result<Self, EmptyRange> {
        if start < end {
            Ok(Self { start, end })
        } else {
            Err(EmptyRange { start, end })
        }
    }

    pub const fn start(self) -> u32 {
        self.start
    }

    pub const fn end(self) -> u32 {
        self.end
    }

    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    /// Never true: ranges are non-empty by construction.
    pub const fn is_empty(self) -> bool {
        false
    }

    /// Whether the two share at least one byte.
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRange {
    start: u32,
    end: u32,
}

impl<'de> Deserialize<'de> for ByteRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawRange::deserialize(deserializer)?;
        Self::new(raw.start, raw.end).map_err(serde::de::Error::custom)
    }
}

/// Where text sits: part `part` of `message` in `exchange`, bytes `range`
/// of that part's text (`Message::part_text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub exchange: ExchangeId,
    pub message: MessageId,
    pub part: u16,
    pub range: ByteRange,
}

impl Location {
    /// Same message and part, and at least one shared byte. The exchange is
    /// not compared: alignment compares reader exchanges on its own.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.message == other.message && self.part == other.part && self.range.overlaps(other.range)
    }
}
