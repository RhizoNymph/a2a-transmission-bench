//! From hits to prediction rows: transmission ids, the strongest match and
//! the checked transmission.
//!
//! **Strongest match** (crosstalk-spec's `MatchClass::strongest_match`,
//! INV-519 `flow.quality.strongest-match`): a confirmed transmission's
//! quality is the strongest class among its content matches, in the order
//! `exact`, `normalized`, `decoded`, `semantic`, with the carrier of the
//! first match of that class in stored order. Matches are stored sorted
//! by `read_at` (the format's rule), so that order is the location order.

use a2a_bench_format::ids::{DetectorAgent, Digest, ExchangeId, TransmissionRef};
use a2a_bench_format::labels::MatchClass;
use a2a_bench_format::predictions::{
    ContentEvidence, Quality, State, Transmission, TransmissionFields,
};

use crate::error::ReferenceError;

/// The BLAKE3 derive-key context of a transmission id.
pub const TRANSMISSION_ID_CONTEXT: &str = "a2a-bench-reference/1 transmission";

/// A class's strength: lower is stronger.
const fn rank(class: MatchClass) -> u8 {
    match class {
        MatchClass::Exact => 0,
        MatchClass::Normalized => 1,
        MatchClass::Decoded => 2,
        MatchClass::Semantic => 3,
    }
}

/// The quality of a transmission holding `matches`, in stored order: its
/// strongest class and the carrier of the first match of that class.
/// `None` when there are no matches.
pub fn strongest(matches: &[ContentEvidence]) -> Option<Quality> {
    let class = matches
        .iter()
        .map(|evidence| evidence.kind.class())
        .min_by_key(|class| rank(*class))?;
    let first = matches
        .iter()
        .find(|evidence| evidence.kind.class() == class)?;
    Some(Quality::Content {
        class,
        carrier: first.carrier,
    })
}

/// A transmission's id, from its identity: the reader exchange, the sender
/// and the route key. `t:` and the keyed BLAKE3 of the three, each followed
/// by a zero byte.
pub fn transmission_ref(
    reader_exchange: ExchangeId,
    sender: &DetectorAgent,
    route_key: &str,
) -> Result<TransmissionRef, ReferenceError> {
    let mut bytes = Vec::new();
    for field in [
        reader_exchange.to_ulid().as_str(),
        sender.as_str(),
        route_key,
    ] {
        bytes.extend_from_slice(field.as_bytes());
        bytes.push(0);
    }
    let digest = Digest::keyed(TRANSMISSION_ID_CONTEXT, &bytes);
    TransmissionRef::new(format!("t:{}", digest.to_hex())).map_err(ReferenceError::TransmissionRef)
}

/// A confirmed transmission of `matches` (non-empty), stored sorted by
/// `read_at` as the format requires (stable, so matches at one location
/// keep their order), with the quality of its strongest match in that
/// order.
pub fn confirmed(
    id: TransmissionRef,
    mut matches: Vec<ContentEvidence>,
) -> Result<Transmission, ReferenceError> {
    matches.sort_by_key(|evidence| evidence.read_at);
    let quality = strongest(&matches);
    Ok(Transmission::new(TransmissionFields {
        id,
        state: State::Confirmed,
        quality,
        matches,
        co_access: Vec::new(),
    })?)
}
