//! A location resolves: its exchange is the world's, its message is in that
//! exchange, the part has text, and the range is inside it on character
//! boundaries.

use std::borrow::Cow;

use super::WorldInputs;
use crate::ids::{ExchangeId, MessageId};
use crate::location::Location;
use crate::message::NoPartText;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocationError {
    #[error("exchange {0} is not in the world")]
    UnknownExchange(ExchangeId),
    #[error("message {message} is not in exchange {exchange}")]
    MessageNotInExchange {
        exchange: ExchangeId,
        message: MessageId,
    },
    #[error("message {message}: {source}")]
    NoText {
        message: MessageId,
        source: NoPartText,
    },
    #[error("range {start}..{end} is past the part's {len} bytes")]
    PastEnd { start: u32, end: u32, len: usize },
    #[error("range {start}..{end} splits a character")]
    SplitsCharacter { start: u32, end: u32 },
}

impl WorldInputs {
    /// The text at `location`.
    pub fn text_at(&self, location: &Location) -> Result<Cow<'_, str>, LocationError> {
        let exchange = self
            .exchange(location.exchange)
            .ok_or(LocationError::UnknownExchange(location.exchange))?;
        if exchange.side_of(location.message).is_none() {
            return Err(LocationError::MessageNotInExchange {
                exchange: location.exchange,
                message: location.message,
            });
        }
        let message =
            self.message(location.message)
                .ok_or(LocationError::MessageNotInExchange {
                    exchange: location.exchange,
                    message: location.message,
                })?;
        let text = message
            .part_text(location.part)
            .map_err(|source| LocationError::NoText {
                message: location.message,
                source,
            })?;
        let (start, end) = (location.range.start(), location.range.end());
        let (from, to) = (
            usize::try_from(start).unwrap_or(usize::MAX),
            usize::try_from(end).unwrap_or(usize::MAX),
        );
        if to > text.len() {
            return Err(LocationError::PastEnd {
                start,
                end,
                len: text.len(),
            });
        }
        if !text.is_char_boundary(from) || !text.is_char_boundary(to) {
            return Err(LocationError::SplitsCharacter { start, end });
        }
        Ok(match text {
            Cow::Borrowed(text) => Cow::Borrowed(&text[from..to]),
            Cow::Owned(text) => Cow::Owned(text[from..to].to_owned()),
        })
    }
}
