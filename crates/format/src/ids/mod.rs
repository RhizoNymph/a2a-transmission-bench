//! Identifiers: dataset, world, agent and label keys, source references,
//! exchange ids (ULIDs) and message ids (BLAKE3 digests).

mod derive;
mod ulid;

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use derive::{DOMAIN, exchange_id};
pub use ulid::{InvalidUlid, ULID_LEN};

/// Why a key is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidKey {
    #[error("{kind} is empty")]
    Empty { kind: &'static str },
    #[error("{kind} holds a control character at byte {at}")]
    Control { kind: &'static str, at: usize },
    #[error("{kind} {text:?} holds a character outside [a-z0-9_/.-]")]
    Charset { kind: &'static str, text: String },
}

fn check_text(kind: &'static str, text: &str) -> Result<(), InvalidKey> {
    if text.is_empty() {
        return Err(InvalidKey::Empty { kind });
    }
    if let Some((at, _)) = text.char_indices().find(|(_, c)| c.is_control()) {
        return Err(InvalidKey::Control { kind, at });
    }
    Ok(())
}

fn check_slug(kind: &'static str, text: &str) -> Result<(), InvalidKey> {
    check_text(kind, text)?;
    let valid = text.bytes().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'/' | b'.')
    });
    if valid {
        Ok(())
    } else {
        Err(InvalidKey::Charset {
            kind,
            text: text.to_owned(),
        })
    }
}

/// A string key with a validity check, serialized as its text.
macro_rules! key {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $check:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(text: impl Into<String>) -> Result<Self, InvalidKey> {
                let text = text.into();
                $check($kind, &text)?;
                Ok(Self(text))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::new(text).map_err(serde::de::Error::custom)
            }
        }
    };
}

key!(
    /// A dataset: `salt`, `ai-village`, `demo-swarm/headline`. Lower-case
    /// ASCII letters, digits and `_ - / .`.
    DatasetId,
    "dataset id",
    check_slug
);
key!(
    /// A world, unique within its dataset.
    WorldKey,
    "world key",
    check_text
);
key!(
    /// An agent, unique within its world.
    AgentKey,
    "agent key",
    check_text
);
key!(
    /// A label row, unique within its export.
    LabelId,
    "label id",
    check_text
);
key!(
    /// An agent as a detector names it, unique within a predictions file's
    /// world.
    DetectorAgent,
    "detector agent",
    check_text
);
key!(
    /// A detector's transmission, unique within a predictions file's world.
    TransmissionRef,
    "transmission ref",
    check_text
);

/// Where in a dataset something came from: a file relative to the dataset
/// root and a JSON-pointer-like path inside it. Exchange ids derive from it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    file: String,
    path: String,
}

impl SourceRef {
    pub fn new(file: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            path: path.into(),
        }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

/// An exchange: 128 bits, written as a ULID. Derived ids carry the
/// exchange's time in their top 48 bits ([`exchange_id`]); recorded ids (a
/// gateway's) are carried as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExchangeId(u128);

impl ExchangeId {
    pub const fn from_raw(raw: u128) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u128 {
        self.0
    }

    pub fn to_ulid(self) -> String {
        ulid::encode(self.0)
    }
}

impl FromStr for ExchangeId {
    type Err = InvalidUlid;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        ulid::decode(text).map(Self)
    }
}

impl fmt::Display for ExchangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        ulid::UlidText(self.0).fmt(f)
    }
}

impl Serialize for ExchangeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_ulid())
    }
}

impl<'de> Deserialize<'de> for ExchangeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A BLAKE3 digest, written as 64 lower-case hex characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

/// Text that is not a digest.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDigest {
    #[error("a digest is 64 hex characters, got {got}")]
    Length { got: usize },
    #[error("character {index} is not lower-case hex")]
    Character { index: usize },
}

impl Digest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The BLAKE3 digest of `bytes` in the derive-key mode under `context`.
    pub fn keyed(context: &str, bytes: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(context);
        hasher.update(bytes);
        Self(*hasher.finalize().as_bytes())
    }

    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0xf)]));
        }
        out
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

impl FromStr for Digest {
    type Err = InvalidDigest;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return Err(InvalidDigest::Length { got: bytes.len() });
        }
        let mut out = [0u8; 32];
        for (at, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
            let high = hex_value(pair[0]).ok_or(InvalidDigest::Character { index: 2 * at })?;
            let low = hex_value(pair[1]).ok_or(InvalidDigest::Character { index: 2 * at + 1 })?;
            out[at] = (high << 4) | low;
        }
        Ok(Self(out))
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A message: the [`Digest`] of its canonical JSON (see
/// [`crate::message::Message`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(Digest);

impl MessageId {
    pub(crate) const fn from_digest(digest: Digest) -> Self {
        Self(digest)
    }

    pub fn digest(&self) -> &Digest {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
