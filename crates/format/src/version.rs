//! The format version, carried by every file's header and the manifest.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The format this crate reads and writes. A reader accepts exactly this.
pub const FORMAT: Format = Format { major: 1 };

/// `a2a-bench/<major>`. Any change to part text, canonical JSON, id
/// derivation, alignment or a row's fields is a new major.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Format {
    major: u32,
}

const PREFIX: &str = "a2a-bench/";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{text:?} is not a2a-bench/<major>")]
pub struct InvalidFormat {
    pub text: String,
}

impl Format {
    pub const fn major(self) -> u32 {
        self.major
    }

    pub fn parse(text: &str) -> Result<Self, InvalidFormat> {
        text.strip_prefix(PREFIX)
            .filter(|major| !major.starts_with('0') || *major == "0")
            .and_then(|major| major.parse().ok())
            .map(|major| Self { major })
            .ok_or_else(|| InvalidFormat {
                text: text.to_owned(),
            })
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.major)
    }
}

impl Serialize for Format {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Format {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}
