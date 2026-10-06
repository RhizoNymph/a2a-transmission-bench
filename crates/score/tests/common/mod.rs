//! Synthetic worlds for the scorer's tests: a builder over the format's
//! types (messages, exchanges, labels), checked as an export would be, and
//! helpers that write a world as the four files of an export and a run.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use a2a_bench_format::check::{WorldInputs, check_labels};
use a2a_bench_format::exchange::{
    AgentDecl, Client, Driven, Exchange, Fidelity, Request, Response, WorldDecl,
};
use a2a_bench_format::files::{
    Coverage, DetectorInfo, ExchangeRow, Exchanges, Labels, LabelsWorld, MessageRow, Messages,
    Predictions, PredictionsHeader, PredictionsWorld, WorldOnly,
};
use a2a_bench_format::ids::{
    AgentKey, DatasetId, Digest, ExchangeId, LabelId, SourceRef, WorldKey, exchange_id,
};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::jsonl::{BasicHeader, FileWriter};
use a2a_bench_format::labels::{
    CarrierKind, ControlFields, ExchangeAgent, ExpectedContent, Label, MatchNeed, NegativeControl,
    NegativeReason, Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::predictions::{Prediction as Row, WorldStatus};
use a2a_bench_format::time::Timestamp;
use a2a_bench_score::World;

pub const DATASET: &str = "synthetic";

pub fn dataset() -> DatasetId {
    DatasetId::new(DATASET).unwrap()
}

pub fn agent_key(name: &str) -> AgentKey {
    AgentKey::new(name).unwrap()
}

pub fn label_id(name: &str) -> LabelId {
    LabelId::new(name).unwrap()
}

pub fn message(body: Body) -> Message {
    Message::new(body).unwrap()
}

pub fn system(text: &str) -> Message {
    message(Body::System(vec![SystemPart::Text { text: text.into() }]))
}

pub fn user(text: &str) -> Message {
    message(Body::User(vec![UserPart::Text { text: text.into() }]))
}

/// An assistant message saying `text`.
pub fn says(text: &str) -> Message {
    message(Body::Assistant(vec![AssistantPart::Text {
        text: text.into(),
    }]))
}

/// An assistant message calling `name` with JSON `arguments`.
pub fn calls(call_id: &str, name: &str, arguments: &str) -> Message {
    message(Body::Assistant(vec![AssistantPart::ToolCall(ToolCall {
        call_id: call_id.into(),
        name: name.into(),
        arguments: ToolArguments::Json(CanonicalJson::canonicalize(arguments).unwrap()),
        execution: ToolExecution::Client,
    })]))
}

/// A tool message returning `text` for `call_id`.
pub fn result(call_id: &str, text: &str) -> Message {
    message(Body::Tool(vec![ToolPart::ToolResult(ToolResult {
        call_id: call_id.into(),
        content: vec![ResultContent::Text { text: text.into() }],
        outcome: ToolOutcome::Success,
    })]))
}

/// Bytes `start..end` of part `part` of `message` in `exchange`.
pub fn at(exchange: ExchangeId, message: &Message, part: u16, start: u32, end: u32) -> Location {
    Location {
        exchange,
        message: message.id(),
        part,
        range: ByteRange::new(start, end).unwrap(),
    }
}

/// The whole text of part `part` of `message` in `exchange`.
pub fn whole(exchange: ExchangeId, message: &Message, part: u16) -> Location {
    let len = u32::try_from(message.part_text(part).unwrap().len()).unwrap();
    at(exchange, message, part, 0, len)
}

pub fn len(text: &str) -> u32 {
    u32::try_from(text.len()).unwrap()
}

pub fn tick(seconds: u64) -> Timestamp {
    Timestamp::from_micros(1_767_225_600_000_000 + seconds * 1_000_000)
}

/// A content label from `from` to `to` at `content` (its text is read from
/// the location when the world is finished, so the caller gives the text).
#[allow(clippy::too_many_arguments)]
pub fn content_fields(
    id: &str,
    from: &AgentKey,
    to: &AgentKey,
    reader: ExchangeId,
    route: Route,
    carrier: CarrierKind,
    text: &str,
    content: Location,
    tier: Tier,
) -> TransmissionFields {
    TransmissionFields {
        id: label_id(id),
        from: from.clone(),
        to: to.clone(),
        sender_exchange: None,
        reader_exchange: reader,
        route,
        carrier,
        content: ExpectedContent {
            text: text.into(),
            at: content,
        },
        needs: MatchNeed::Exact,
        tier,
        source: SourceRef::new("f", format!("/labels/{id}")),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn control(
    id: &str,
    from: &AgentKey,
    to: &AgentKey,
    reader: Option<ExchangeId>,
    location: Option<Location>,
    origin: Option<Location>,
    reason: NegativeReason,
    tier: Tier,
) -> Label {
    Label::NegativeControl(
        NegativeControl::new(ControlFields {
            id: label_id(id),
            from: from.clone(),
            to: to.clone(),
            reader_exchange: reader,
            at: location,
            origin,
            text: None,
            reason,
            tier,
            source: SourceRef::new("f", format!("/controls/{id}")),
        })
        .unwrap(),
    )
}

/// A world being built: agents, exchanges (any order; sorted by time at
/// the end) and labels.
pub struct Draft {
    key: WorldKey,
    agents: Vec<AgentDecl>,
    messages: BTreeMap<a2a_bench_format::ids::MessageId, Message>,
    exchanges: Vec<(Exchange, AgentKey)>,
    labels: Vec<Label>,
}

impl Draft {
    pub fn new(world: &str) -> Self {
        Self {
            key: WorldKey::new(world).unwrap(),
            agents: Vec::new(),
            messages: BTreeMap::new(),
            exchanges: Vec::new(),
            labels: Vec::new(),
        }
    }

    pub fn agent(&mut self, name: &str) -> AgentKey {
        self.declare(name, Driven::Model)
    }

    pub fn scripted(&mut self, name: &str) -> AgentKey {
        self.declare(name, Driven::Scripted)
    }

    fn declare(&mut self, name: &str, driven: Driven) -> AgentKey {
        let key = agent_key(name);
        self.agents.push(AgentDecl {
            key: key.clone(),
            driven,
            model: None,
        });
        key
    }

    /// An exchange of `agent` at `second`, with `request` and `response`.
    pub fn exchange(
        &mut self,
        agent: &AgentKey,
        second: u64,
        request: &[&Message],
        response: &[&Message],
    ) -> ExchangeId {
        for message in request.iter().chain(response) {
            self.messages.insert(message.id(), (*message).clone());
        }
        let source = SourceRef::new(
            "fixture.json",
            format!("/{}/{}", self.key, self.exchanges.len()),
        );
        let when = tick(second);
        let id = exchange_id(&dataset(), &source, when);
        let exchange = Exchange {
            id,
            at_us: when,
            client: Client {
                credential: format!("k:{agent}"),
                session: None,
                turn: None,
                vendor: None,
                model: None,
            },
            request: Request {
                messages: request.iter().map(|m| m.id()).collect(),
                tools: None,
            },
            response: Response {
                messages: response.iter().map(|m| m.id()).collect(),
                stop: None,
            },
            fidelity: Fidelity::Exact,
            source,
        };
        self.exchanges.push((exchange, agent.clone()));
        id
    }

    pub fn expect(&mut self, label: Label) {
        self.labels.push(label);
    }

    /// The world, checked as an export's world is: inputs, then labels.
    pub fn finish(mut self, coverage: Coverage) -> Built {
        self.exchanges.sort_by_key(|(exchange, _)| exchange.at_us);
        let mut labels: Vec<Label> = self
            .exchanges
            .iter()
            .map(|(exchange, agent)| {
                Label::ExchangeAgent(ExchangeAgent {
                    exchange: exchange.id,
                    agent: agent.clone(),
                })
            })
            .collect();
        labels.extend(self.labels);
        let decl = WorldDecl {
            key: self.key.clone(),
            agents: self.agents,
        };
        let messages: Vec<Message> = self.messages.into_values().collect();
        let exchanges: Vec<Exchange> = self.exchanges.into_iter().map(|(e, _)| e).collect();
        let inputs = WorldInputs::new(&self.key, messages.clone(), decl.clone(), exchanges.clone())
            .unwrap_or_else(|e| panic!("inputs: {e}"));
        check_labels(&inputs, &labels).unwrap_or_else(|e| panic!("labels: {e}"));
        Built {
            dataset: dataset(),
            key: self.key,
            decl,
            messages,
            exchanges,
            inputs,
            labels,
            coverage,
        }
    }
}

/// A finished, checked world.
pub struct Built {
    pub dataset: DatasetId,
    pub key: WorldKey,
    pub decl: WorldDecl,
    pub messages: Vec<Message>,
    pub exchanges: Vec<Exchange>,
    pub inputs: WorldInputs,
    pub labels: Vec<Label>,
    pub coverage: Coverage,
}

impl Built {
    pub fn world(&self) -> World<'_> {
        World {
            dataset: &self.dataset,
            inputs: &self.inputs,
            labels: &self.labels,
            coverage: self.coverage,
        }
    }
}

pub fn complete() -> Coverage {
    Coverage::Complete {
        tier: Tier::Construction,
    }
}

/// The four files of an export and a run over `worlds`.
pub struct Files {
    pub messages: Vec<u8>,
    pub exchanges: Vec<u8>,
    pub labels: Vec<u8>,
    pub predictions: Vec<u8>,
}

pub fn detector() -> DetectorInfo {
    DetectorInfo {
        name: "test-detector".into(),
        version: "0".into(),
        variant: "default".into(),
        config_digest: None,
    }
}

/// Writes `worlds` and, for each, its predictions world status and rows.
pub fn files(worlds: &[(&Built, WorldStatus, Vec<Row>)]) -> Files {
    files_for(worlds, Digest::from_bytes([0; 32]))
}

/// [`files`] with predictions naming `manifest_digest`.
pub fn files_for(worlds: &[(&Built, WorldStatus, Vec<Row>)], manifest_digest: Digest) -> Files {
    let mut messages =
        FileWriter::<Messages, _>::new(Vec::new(), &BasicHeader::new::<Messages>(dataset()))
            .unwrap();
    let mut exchanges =
        FileWriter::<Exchanges, _>::new(Vec::new(), &BasicHeader::new::<Exchanges>(dataset()))
            .unwrap();
    let mut labels =
        FileWriter::<Labels, _>::new(Vec::new(), &BasicHeader::new::<Labels>(dataset())).unwrap();
    let header = PredictionsHeader::new(dataset(), detector(), manifest_digest);
    let mut predictions = FileWriter::<Predictions, _>::new(Vec::new(), &header).unwrap();
    for (built, status, rows) in worlds {
        messages
            .world(&WorldOnly {
                key: built.key.clone(),
            })
            .unwrap();
        for message in &built.messages {
            messages.row(&MessageRow::Message(message.clone())).unwrap();
        }
        exchanges.world(&built.decl).unwrap();
        for exchange in &built.exchanges {
            exchanges
                .row(&ExchangeRow::Exchange(exchange.clone()))
                .unwrap();
        }
        labels
            .world(&LabelsWorld {
                key: built.key.clone(),
                coverage: built.coverage,
            })
            .unwrap();
        for label in &built.labels {
            labels.row(label).unwrap();
        }
        predictions
            .world(&PredictionsWorld {
                key: built.key.clone(),
                status: status.clone(),
            })
            .unwrap();
        for row in rows {
            predictions.row(row).unwrap();
        }
    }
    Files {
        messages: messages.finish().unwrap().0,
        exchanges: exchanges.finish().unwrap().0,
        labels: labels.finish().unwrap().0,
        predictions: predictions.finish().unwrap().0,
    }
}
