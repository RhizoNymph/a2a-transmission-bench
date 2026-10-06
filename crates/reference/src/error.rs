//! Why the reference could not process a world.

use a2a_bench_format::ids::{ExchangeId, InvalidKey, MessageId};
use a2a_bench_format::predictions::InvalidTransmission;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error("credential {credential:?} cannot name a detector agent: {source}")]
    Credential {
        credential: String,
        source: InvalidKey,
    },
    #[error("exchange {exchange} names message {message}, which the world does not hold")]
    MissingMessage {
        exchange: ExchangeId,
        message: MessageId,
    },
    #[error("a transmission id is not a valid reference: {0}")]
    TransmissionRef(InvalidKey),
    #[error("a transmission could not be built: {0}")]
    Transmission(#[from] InvalidTransmission),
}
