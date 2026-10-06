//! Locations in a reader's exchange, and where a message is first carried.

use a2a_bench_format::ids::{ExchangeId, MessageId};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::Message;

use crate::AgentDojoError;

/// Bytes `start..end` of part `part` of `message`, in `exchange`.
pub fn location(
    exchange: ExchangeId,
    message: &Message,
    part: u16,
    start: usize,
    end: usize,
) -> Result<Location, AgentDojoError> {
    let start = u32::try_from(start).map_err(|_| AgentDojoError::Offset)?;
    let end = u32::try_from(end).map_err(|_| AgentDojoError::Offset)?;
    Ok(Location {
        exchange,
        message: message.id(),
        part,
        range: ByteRange::new(start, end)?,
    })
}

/// The whole text of part `part` of `message`, in `exchange`; `None` when
/// the part has no text or it is empty (crosstalk-eval's `whole_part`
/// refuses both).
pub fn whole_part(exchange: ExchangeId, message: &Message, part: u16) -> Option<Location> {
    let text = message.part_text(part).ok()?;
    location(exchange, message, part, 0, text.len()).ok()
}

/// The victim's exchanges, each with the conversation prefix its request
/// carries.
pub struct Carriers<'a> {
    /// (assistant message index, exchange): exchange `k`'s request is
    /// messages `..k`. In message order, so in time order.
    pub exchanges: &'a [(usize, ExchangeId)],
    pub conversation: &'a [Message],
}

impl Carriers<'_> {
    /// The victim's first exchange whose request carries `message` (by id,
    /// so a repeated message counts from its first copy). This is where
    /// crosstalk's golden export places a control that names a message but
    /// no exchange.
    pub fn first(&self, message: MessageId) -> Option<ExchangeId> {
        let first = self
            .conversation
            .iter()
            .position(|held| held.id() == message)?;
        self.exchanges
            .iter()
            .find(|(index, _)| *index > first)
            .map(|(_, exchange)| *exchange)
    }
}
