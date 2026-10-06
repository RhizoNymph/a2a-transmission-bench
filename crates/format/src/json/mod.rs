//! JSON with exact numbers, and its canonical text.
//!
//! The canonical text is RFC 8785 except that numbers keep their exact
//! decimal value instead of going through an IEEE double, so an integer
//! beyond 2^53 keeps every digit. This is the same rule as crosstalk-spec's
//! `CanonicalJson`, and the parity tests check the two agree byte for byte.
//! `serde_json` would round such numbers, so this module has its own parser
//! ([`Json::parse`]), value ([`Json`], numbers as [`Number`]) and writer.

mod number;
mod parse;
mod write;

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use number::{MAX_EXPONENT_DIGITS, Number};
pub use parse::{JsonError, MAX_DEPTH};

/// A JSON value with exact numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Json>),
    /// Members in the order their names first appeared; a repeated name
    /// holds its last value. The canonical text sorts them.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Parses JSON text strictly: RFC 8259 with no extensions, unpaired
    /// surrogate escapes refused, at most [`MAX_DEPTH`] levels deep, and a
    /// repeated member name keeping its last value.
    pub fn parse(text: &str) -> Result<Self, JsonError> {
        parse::parse(text)
    }

    /// [`Json::parse`] for bytes, which must be UTF-8.
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, JsonError> {
        parse::parse_bytes(bytes)
    }

    /// The canonical text of this value.
    pub fn canonical(&self) -> CanonicalJson {
        let mut out = String::new();
        write::write(self, &mut out);
        CanonicalJson(out)
    }
}

/// JSON text in canonical form. Only [`CanonicalJson::canonicalize`] and
/// [`Json::canonical`] make one, and deserializing checks the text is
/// already canonical, so every value of this type is canonical.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalJson(String);

/// Text that parses but is not in canonical form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalJsonError {
    #[error("not JSON: {0}")]
    Json(#[from] JsonError),
    #[error("JSON text is not canonical")]
    NotCanonical,
}

impl CanonicalJson {
    /// The canonical form of any JSON text.
    pub fn canonicalize(text: &str) -> Result<Self, JsonError> {
        Json::parse(text).map(|value| value.canonical())
    }

    /// `text`, which must already be canonical.
    pub fn from_canonical(text: String) -> Result<Self, CanonicalJsonError> {
        let canonical = Self::canonicalize(&text)?;
        if canonical.0 == text {
            Ok(canonical)
        } else {
            Err(CanonicalJsonError::NotCanonical)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for CanonicalJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for CanonicalJson {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_canonical(text).map_err(serde::de::Error::custom)
    }
}
