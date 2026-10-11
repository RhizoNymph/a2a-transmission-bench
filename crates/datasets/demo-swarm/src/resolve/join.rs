//! Joining a row's reader and writer to captured exchanges: by session and
//! tool use id alone, then the BLAKE3 of the content. The truth's turns
//! are only cross-checked against the exchange found (`turn_mismatch`).
//!
//! A reader's tool result is carried again, as history, by every later
//! request of its session, so the reader is the session's *first* exchange
//! (in ordinal order) whose request holds it. A turn is never trusted to
//! pick among the carrying exchanges: when the session's turn counter ran
//! ahead of the capture (a request refused before the gateway, or one not
//! captured), the exchange at the stated turn carries the same result with
//! the same hash, and a turn-first join would land one exchange late
//! without a mismatch to show for it.

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::ids::ExchangeId;

use super::Resolver;
use crate::diagnostics::{Effect, JoinFailure, RowKind, Side};
use crate::locate::{FoundCall, FoundResult, LocateError, tool_result, write_call};
use crate::schema::{Delivery, HexDigest};
use crate::sessions::Session;

/// Where a join looked: the truth line, the row and side, and the session
/// and turn it named.
struct Site<'s> {
    line: usize,
    row: RowKind,
    side: Side,
    session_id: &'s str,
    session: &'s Session,
    turn: u32,
}

/// A joined read: the reader's exchange and the tool result in it.
pub(crate) struct ReadJoin {
    pub exchange: ExchangeId,
    pub found: FoundResult,
}

/// What a session holds of one tool use.
enum Held<'e, T> {
    /// The first exchange holding it, what was found there, and how many
    /// exchanges hold it.
    Found {
        exchange: &'e Exchange,
        found: T,
        holding: usize,
    },
    NotFound,
    /// An exchange the search reached could not be read.
    Failed {
        exchange: &'e Exchange,
        error: LocateError,
    },
}

/// The session's first exchange, in ordinal order, for which `find` finds
/// something; with `count_all`, every exchange is searched to count those
/// holding it, else the search stops at the first.
fn first_holding<'e, T>(
    session: &'e Session,
    count_all: bool,
    mut find: impl FnMut(&Exchange) -> Result<Option<T>, LocateError>,
) -> Held<'e, T> {
    let mut first: Option<(&Exchange, T)> = None;
    let mut holding = 0;
    for exchange in &session.exchanges {
        match find(exchange) {
            Ok(Some(found)) => {
                holding += 1;
                if first.is_none() {
                    first = Some((exchange, found));
                }
                if !count_all {
                    break;
                }
            }
            Ok(None) => {}
            // Past the first holding exchange, a failure only ends the count.
            Err(_) if first.is_some() => break,
            Err(error) => return Held::Failed { exchange, error },
        }
    }
    match first {
        Some((exchange, found)) => Held::Found {
            exchange,
            found,
            holding,
        },
        None => Held::NotFound,
    }
}

impl Resolver<'_> {
    /// Joins a read: the first exchange of `session` whose request holds
    /// the tool result `call`, checked against `digest` when given. `turn`
    /// is the truth's, only cross-checked.
    pub(crate) fn read(
        &mut self,
        line: usize,
        row: RowKind,
        session_id: &str,
        turn: u32,
        call: &str,
        digest: Option<HexDigest>,
    ) -> Option<ReadJoin> {
        let sessions = self.sessions;
        let Some(session) = sessions.get(session_id) else {
            self.report(
                line,
                row,
                Side::Reader,
                JoinFailure::UnknownSession {
                    session: session_id.to_owned(),
                },
                Effect::Dropped,
            );
            return None;
        };
        let site = Site {
            line,
            row,
            side: Side::Reader,
            session_id,
            session,
            turn,
        };
        let messages = self.messages;
        let (exchange, found) = match first_holding(session, false, |exchange| {
            tool_result(exchange, call, messages)
        }) {
            Held::Found {
                exchange, found, ..
            } => (exchange, found),
            Held::NotFound => {
                self.not_found(&site, call, Effect::Dropped);
                return None;
            }
            Held::Failed { exchange, error } => {
                self.body_failure(line, row, Side::Reader, exchange, &error, Effect::Dropped);
                return None;
            }
        };
        let id = exchange.id;
        if digest.is_some_and(|digest| digest != found.digest()) {
            self.report(
                line,
                row,
                Side::Reader,
                JoinFailure::HashMismatch {
                    session: session_id.to_owned(),
                    turn,
                    tool_use_id: call.to_owned(),
                    exchange: id,
                },
                Effect::Dropped,
            );
            return None;
        }
        self.check_turn(&site, id, Effect::Kept);
        Some(ReadJoin {
            exchange: id,
            found,
        })
    }

    /// Joins a transmission's write: the exchange whose response holds the
    /// `PUT` tool use `writer_tool_use_id` (the first, reported, when more
    /// than one does), or `None` (and a diagnostic) when it cannot be
    /// joined.
    pub(crate) fn write(&mut self, line: usize, row: &Delivery) -> Option<ExchangeId> {
        let kind = RowKind::Transmission;
        let effect = Effect::KeptWithoutSender;
        let sessions = self.sessions;
        let session_id = &row.writer_session;
        let call = &row.writer_tool_use_id;
        let turn = row.writer_turn;
        let Some(session) = sessions.get(session_id) else {
            self.report(
                line,
                kind,
                Side::Writer,
                JoinFailure::UnknownSession {
                    session: session_id.clone(),
                },
                effect,
            );
            return None;
        };
        let site = Site {
            line,
            row: kind,
            side: Side::Writer,
            session_id,
            session,
            turn,
        };
        let messages = self.messages;
        let (exchange, found, holding) = match first_holding(session, true, |exchange| {
            write_call(exchange, call, messages)
        }) {
            Held::Found {
                exchange,
                found,
                holding,
            } => (exchange, found, holding),
            Held::NotFound => {
                self.not_found(&site, call, effect);
                return None;
            }
            Held::Failed { exchange, error } => {
                self.body_failure(line, kind, Side::Writer, exchange, &error, effect);
                return None;
            }
        };
        let id = exchange.id;
        if holding > 1 {
            self.report(
                line,
                kind,
                Side::Writer,
                JoinFailure::ToolUseRepeated {
                    session: session_id.clone(),
                    tool_use_id: call.clone(),
                    exchanges: holding,
                    exchange: id,
                },
                Effect::Noted,
            );
        }
        let body = match found {
            FoundCall::Put { body, .. } => body,
            FoundCall::NotAPut => {
                self.report(
                    line,
                    kind,
                    Side::Writer,
                    JoinFailure::NotAPut {
                        session: session_id.clone(),
                        tool_use_id: call.clone(),
                        exchange: id,
                    },
                    effect,
                );
                return None;
            }
        };
        if HexDigest::blake3_of(body.as_bytes()) != row.content.blake3 {
            self.report(
                line,
                kind,
                Side::Writer,
                JoinFailure::HashMismatch {
                    session: session_id.clone(),
                    turn,
                    tool_use_id: call.clone(),
                    exchange: id,
                },
                effect,
            );
            return None;
        }
        self.check_turn(&site, id, Effect::Kept);
        Some(id)
    }

    /// Reports the truth's turn when it is not the found exchange's
    /// ordinal; the join stands either way.
    fn check_turn(&mut self, site: &Site<'_>, found: ExchangeId, effect: Effect) {
        let found_turn = site.session.ordinal(found).unwrap_or(u32::MAX);
        if found_turn != site.turn {
            self.report(
                site.line,
                site.row,
                site.side,
                JoinFailure::TurnMismatch {
                    session: site.session_id.to_owned(),
                    turn: site.turn,
                    found_turn,
                    exchange: found,
                },
                effect,
            );
        }
    }

    fn not_found(&mut self, site: &Site<'_>, call: &str, effect: Effect) {
        self.report(
            site.line,
            site.row,
            site.side,
            JoinFailure::ToolUseMissing {
                session: site.session_id.to_owned(),
                turn: site.turn,
                tool_use_id: call.to_owned(),
            },
            effect,
        );
    }

    fn body_failure(
        &mut self,
        line: usize,
        row: RowKind,
        side: Side,
        exchange: &Exchange,
        error: &LocateError,
        effect: Effect,
    ) {
        self.report(
            line,
            row,
            side,
            JoinFailure::Body {
                exchange: exchange.id,
                reason: error.to_string(),
            },
            effect,
        );
    }
}
