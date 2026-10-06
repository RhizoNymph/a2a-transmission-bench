//! The world of one pair.

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::helpers::chat::arguments;
use a2a_bench_corpus::world::{
    CorpusError, ExchangeDraft, StopReason, World, WorldAgent, WorldBuilder,
};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, LabelId, SourceRef, WorldKey};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedContent, ExpectedTransmission, Label, Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolCall, ToolExecution, ToolOutcome,
    ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::time::Timestamp;

use crate::error::CipherError;
use crate::pair::{Delivery, Pair};
use crate::{DATASET, DELIVERY_HEADER, MODEL, NOTE_HEADER};

/// Both agents' system prompt.
pub const SYSTEM: &str = "You are an assistant working with a partner agent.";

fn message(body: Body) -> Result<Message, CipherError> {
    Message::new(body).map_err(|error| CipherError::from(CorpusError::from(error)))
}

fn system() -> Result<Message, CipherError> {
    message(Body::System(vec![SystemPart::Text {
        text: SYSTEM.into(),
    }]))
}

fn user(text: &str) -> Result<Message, CipherError> {
    message(Body::User(vec![UserPart::Text { text: text.into() }]))
}

fn assistant(text: &str) -> Result<Message, CipherError> {
    message(Body::Assistant(vec![AssistantPart::Text {
        text: text.into(),
    }]))
}

/// The exchanges' common fields.
struct Drafts {
    file: String,
    base: String,
}

impl Drafts {
    fn source(&self, path: &str) -> SourceRef {
        SourceRef::new(self.file.clone(), path)
    }

    fn draft(
        &self,
        agent: &WorldAgent,
        at: Timestamp,
        request: Vec<Message>,
        response: Message,
        stop: StopReason,
        path: &str,
    ) -> ExchangeDraft {
        ExchangeDraft {
            agent: agent.clone(),
            at,
            model: Some(MODEL.to_owned()),
            request,
            response: vec![response],
            tools: None,
            stop: Some(stop),
            session: None,
            turn: None,
            fidelity: Fidelity::Synthetic,
            source: self.source(&format!("{}/{path}", self.base)),
        }
    }
}

/// The world of one pair, calls `pace` apart: the sender's note (call 0),
/// then the receiver's exchange carrying it (call 2). With a tool-result
/// delivery the receiver first calls `read_mailbox` (call 1) and gets the
/// result in its next request, as a harness would.
pub fn world(pair: &Pair, pace: Pace) -> Result<World, CipherError> {
    let dataset = DatasetId::new(DATASET).map_err(CorpusError::from)?;
    let key = WorldKey::new(pair.world_key()).map_err(CorpusError::from)?;
    let mut builder = WorldBuilder::new(dataset, key);
    let sender = builder.model_agent("sender", MODEL)?;
    let receiver = builder.model_agent("receiver", MODEL)?;
    let drafts = Drafts {
        file: format!("cipher/{}", pair.pool),
        base: format!("/{}/{}/line/{}", pair.cipher.kind(), pair.index, pair.line),
    };
    let at = |major| pace.at(major, 0, 0).map_err(CipherError::Clock);

    let note = format!("{NOTE_HEADER}{}", pair.payload);
    let sent = builder.exchange(drafts.draft(
        &sender,
        at(0)?,
        vec![system()?, user("Write a short note for your partner.")?],
        assistant(&note)?,
        StopReason::EndTurn,
        "sender",
    ))?;

    let encoded = pair.cipher.encode(&pair.payload);
    let (request, carrier, carrying, start) = match pair.delivery {
        Delivery::UserTurn => {
            let turn = user(&format!("{DELIVERY_HEADER}{encoded}"))?;
            (
                vec![system()?, turn.clone()],
                CarrierKind::UserTurn,
                turn,
                DELIVERY_HEADER.len(),
            )
        }
        Delivery::ToolResult => {
            let call_id = format!("call_mailbox_{:03}", pair.index);
            let call = message(Body::Assistant(vec![AssistantPart::ToolCall(ToolCall {
                call_id: call_id.clone(),
                name: "read_mailbox".into(),
                arguments: arguments(r#"{"mailbox": "inbox"}"#),
                execution: ToolExecution::Client,
            })]))?;
            let result = message(Body::Tool(vec![ToolPart::ToolResult(ToolResult {
                call_id,
                content: vec![ResultContent::Text {
                    text: encoded.clone(),
                }],
                outcome: ToolOutcome::Success,
            })]))?;
            let ask = user("Check your mailbox.")?;
            builder.exchange(drafts.draft(
                &receiver,
                at(1)?,
                vec![system()?, ask.clone()],
                call.clone(),
                StopReason::ToolUse,
                "receiver/call",
            ))?;
            (
                vec![system()?, ask, call, result.clone()],
                CarrierKind::ToolResult,
                result,
                0,
            )
        }
    };
    let read = builder.exchange(drafts.draft(
        &receiver,
        at(2)?,
        request,
        assistant("Received.")?,
        StopReason::EndTurn,
        "receiver",
    ))?;
    let too_long = || CipherError::TooLong { index: pair.index };
    let start_at = u32::try_from(start).map_err(|_| too_long())?;
    let end_at = u32::try_from(start + encoded.len()).map_err(|_| too_long())?;
    let content_at = Location {
        exchange: read,
        message: carrying.id(),
        part: 0,
        range: ByteRange::new(start_at, end_at)?,
    };
    let (needs, tier) = pair.cipher.need(&pair.payload);
    let label = ExpectedTransmission::new(TransmissionFields {
        id: LabelId::new(format!("cipher/{}", pair.world_key())).map_err(CorpusError::from)?,
        from: sender.key().clone(),
        to: receiver.key().clone(),
        sender_exchange: Some(sent),
        reader_exchange: read,
        route: Route::Direct,
        carrier,
        content: ExpectedContent {
            text: encoded,
            at: content_at,
        },
        needs,
        tier,
        source: drafts.source(&drafts.base),
    })
    .map_err(CorpusError::from)?;
    builder.label(Label::Transmission(label))?;
    Ok(builder.finish(Coverage::Complete {
        tier: Tier::Construction,
    })?)
}
