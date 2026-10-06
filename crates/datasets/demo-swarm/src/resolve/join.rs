//! Joining a row's reader and writer to captured exchanges: session, turn,
//! tool use id, then the BLAKE3 of the content.

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

/// The session's exchange at `turn` first, then every other in order.
fn candidates(session: &Session, turn: u32) -> impl Iterator<Item = &Exchange> {
    let at_turn = session.at(turn);
    let skip = at_turn.map(|at| at.id);
    at_turn.into_iter().chain(
        session
            .exchanges
            .iter()
            .filter(move |exchange| Some(exchange.id) != skip),
    )
}

impl Resolver<'_> {
    /// Joins a read: the tool result `call` at `turn` of `session`, checked
    /// against `digest` when given.
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
        for exchange in candidates(session, turn) {
            let found = match tool_result(exchange, call, self.messages) {
                Ok(Some(found)) => found,
                Ok(None) => continue,
                Err(error) => {
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
            let site = Site {
                line,
                row,
                side: Side::Reader,
                session_id,
                session,
                turn,
            };
            self.check_turn(&site, id, Effect::Kept);
            return Some(ReadJoin {
                exchange: id,
                found,
            });
        }
        let site = Site {
            line,
            row,
            side: Side::Reader,
            session_id,
            session,
            turn,
        };
        self.not_found(&site, call, Effect::Dropped);
        None
    }

    /// Joins a transmission's write: the sender's exchange, or `None` (and
    /// a diagnostic) when it cannot be joined.
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
        for exchange in candidates(session, turn) {
            let id = exchange.id;
            let body = match write_call(exchange, call, self.messages) {
                Ok(Some(FoundCall::Put { body, .. })) => body,
                Ok(Some(FoundCall::NotAPut)) => {
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
                Ok(None) => continue,
                Err(error) => {
                    self.body_failure(line, kind, Side::Writer, exchange, &error, effect);
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
            let site = Site {
                line,
                row: kind,
                side: Side::Writer,
                session_id,
                session,
                turn,
            };
            self.check_turn(&site, id, Effect::Kept);
            return Some(id);
        }
        let site = Site {
            line,
            row: kind,
            side: Side::Writer,
            session_id,
            session,
            turn,
        };
        self.not_found(&site, call, effect);
        None
    }

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
        let exchanges = site.session.exchanges.len();
        let failure = if usize::try_from(site.turn).map_or(true, |turn| turn >= exchanges) {
            JoinFailure::TurnOutOfRange {
                session: site.session_id.to_owned(),
                turn: site.turn,
                exchanges,
            }
        } else {
            JoinFailure::ToolUseMissing {
                session: site.session_id.to_owned(),
                turn: site.turn,
                tool_use_id: call.to_owned(),
            }
        };
        self.report(site.line, site.row, site.side, failure, effect);
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
