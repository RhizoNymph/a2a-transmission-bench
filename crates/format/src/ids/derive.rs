//! Deterministic exchange ids, bit for bit those crosstalk-eval derives.
//!
//! An id is the first 16 bytes (big-endian) of
//!
//! ```text
//! BLAKE3("crosstalk-eval/v1" 0x00 "exchange" 0x00 dataset 0x00 file 0x00 path)
//! ```
//!
//! with the top 48 bits replaced by the exchange's time in milliseconds
//! (`at_us / 1000`), as a ULID's are, so ids sort by time. The domain string
//! keeps crosstalk-eval's name on purpose: format v1 fixes it so the same
//! exchange has the same id in ct-eval and in the bench, and outputs can be
//! diffed id by id. Changing it is a format major.

use super::{DatasetId, ExchangeId, SourceRef};
use crate::time::Timestamp;

/// The derivation's domain separator (see the module docs).
pub const DOMAIN: &str = "crosstalk-eval/v1";

fn digest(kind: &str, dataset: &DatasetId, parts: &[&str]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN.as_bytes());
    for field in [kind, dataset.as_str()]
        .into_iter()
        .chain(parts.iter().copied())
    {
        hasher.update(&[0]);
        hasher.update(field.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn derive(kind: &str, dataset: &DatasetId, parts: &[&str], at: Option<Timestamp>) -> u128 {
    let bytes = digest(kind, dataset, parts);
    let mut head = [0u8; 16];
    head.copy_from_slice(&bytes[..16]);
    let raw = u128::from_be_bytes(head);
    match at {
        None => raw,
        Some(at) => {
            let millis = u128::from(at.as_micros() / 1000) & ((1u128 << 48) - 1);
            (millis << 80) | (raw & ((1u128 << 80) - 1))
        }
    }
}

/// The id of the exchange `source` names, made at `at`.
pub fn exchange_id(dataset: &DatasetId, source: &SourceRef, at: Timestamp) -> ExchangeId {
    ExchangeId::from_raw(derive(
        "exchange",
        dataset,
        &[source.file(), source.path()],
        Some(at),
    ))
}
