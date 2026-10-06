//! Locations of labelled text, checked against the message they name.

use a2a_bench_format::ids::ExchangeId;
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LocationError {
    #[error("empty range {start}..{end}")]
    Empty { start: u32, end: u32 },
    #[error("part {part} of the message has no text")]
    NoText { part: u16 },
    #[error("range {start}..{end} is outside the part's {len} bytes or splits a character")]
    OutOfText { start: u32, end: u32, len: usize },
}

/// `[start, end)` of part `part` of `message`, in `exchange`: the part has
/// text and the range is non-empty and falls inside it on character
/// boundaries.
pub fn in_message(
    exchange: ExchangeId,
    message: &Message,
    part: u16,
    start: u32,
    end: u32,
) -> Result<Location, LocationError> {
    let range = ByteRange::new(start, end).map_err(|_| LocationError::Empty { start, end })?;
    let text = message
        .part_text(part)
        .map_err(|_| LocationError::NoText { part })?;
    if text.get(start as usize..end as usize).is_none() {
        return Err(LocationError::OutOfText {
            start,
            end,
            len: text.len(),
        });
    }
    Ok(Location {
        exchange,
        message: message.id(),
        part,
        range,
    })
}
