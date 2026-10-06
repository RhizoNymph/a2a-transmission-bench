//! Shared helpers: small synthetic worlds built by hand. Each agent calls
//! with its own credential (`k:<name>`) unless a test says otherwise.

#![allow(dead_code)]

use std::collections::BTreeMap;

use a2a_bench_format::check::{WorldInputs, check_predictions};
use a2a_bench_format::exchange::{
    AgentDecl, Client, Driven, Exchange, Fidelity, Request, Response, WorldDecl,
};
use a2a_bench_format::ids::{
    AgentKey, DatasetId, DetectorAgent, ExchangeId, MessageId, SourceRef, TransmissionRef,
    WorldKey, exchange_id,
};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::predictions::{ContentEvidence, Prediction, Quality};
use a2a_bench_format::time::Timestamp;
use a2a_bench_reference::{ReferenceConfig, WorldOutput, run};

pub fn dataset() -> DatasetId {
    DatasetId::new("synthetic").unwrap_or_else(|e| panic!("{e}"))
}

pub fn message(body: Body) -> Message {
    Message::new(body).unwrap_or_else(|e| panic!("{e}"))
}

pub fn system(text: &str) -> Message {
    message(Body::System(vec![SystemPart::Text { text: text.into() }]))
}

pub fn user(text: &str) -> Message {
    message(Body::User(vec![UserPart::Text { text: text.into() }]))
}

pub fn says(text: &str) -> Message {
    message(Body::Assistant(vec![AssistantPart::Text {
        text: text.into(),
    }]))
}

/// An assistant message making one client tool call; arguments that are not
/// JSON are kept verbatim.
pub fn calls(id: &str, name: &str, arguments: &str) -> Message {
    let arguments = match CanonicalJson::canonicalize(arguments) {
        Ok(json) => ToolArguments::Json(json),
        Err(_) => ToolArguments::Invalid(arguments.into()),
    };
    message(Body::Assistant(vec![AssistantPart::ToolCall(ToolCall {
        call_id: id.into(),
        name: name.into(),
        arguments,
        execution: ToolExecution::Client,
    })]))
}

pub fn result(call_id: &str, text: &str) -> Message {
    message(Body::Tool(vec![ToolPart::ToolResult(ToolResult {
        call_id: call_id.into(),
        content: vec![ResultContent::Text { text: text.into() }],
        outcome: ToolOutcome::Unknown,
    })]))
}

pub fn at(tick: u64) -> Timestamp {
    Timestamp::from_micros(1_767_225_600_000_000 + tick * 1_000_000)
}

pub fn detector_agent(name: &str) -> DetectorAgent {
    DetectorAgent::new(name).unwrap_or_else(|e| panic!("{e}"))
}

/// The detector agent the reference names an agent calling with its own
/// credential.
pub fn agent_of(name: &str) -> DetectorAgent {
    detector_agent(&format!("k:{name}"))
}

/// Builds one world's inputs.
pub struct WorldBuilder {
    key: WorldKey,
    agents: Vec<AgentDecl>,
    messages: BTreeMap<MessageId, Message>,
    exchanges: Vec<Exchange>,
}

impl WorldBuilder {
    pub fn new(agents: &[&str]) -> Self {
        Self {
            key: WorldKey::new("w").unwrap_or_else(|e| panic!("{e}")),
            agents: agents
                .iter()
                .map(|name| AgentDecl {
                    key: AgentKey::new(*name).unwrap_or_else(|e| panic!("{e}")),
                    driven: Driven::Model,
                    model: None,
                })
                .collect(),
            messages: BTreeMap::new(),
            exchanges: Vec::new(),
        }
    }

    /// The same builder for a world named `key`.
    pub fn named(mut self, key: &str) -> Self {
        self.key = WorldKey::new(key).unwrap_or_else(|e| panic!("{e}"));
        self
    }

    /// Adds a model-driven agent.
    pub fn agent(&mut self, name: &str) {
        self.agents.push(AgentDecl {
            key: AgentKey::new(name).unwrap_or_else(|e| panic!("{e}")),
            driven: Driven::Model,
            model: None,
        });
    }

    /// An exchange of `agent`, calling with credential `k:<agent>`, at tick `tick`.
    pub fn exchange(
        &mut self,
        agent: &str,
        tick: u64,
        request: Vec<Message>,
        response: Vec<Message>,
    ) -> ExchangeId {
        self.exchange_with(&format!("k:{agent}"), agent, tick, request, response)
    }

    /// An exchange of `agent` calling with `credential`.
    pub fn exchange_with(
        &mut self,
        credential: &str,
        agent: &str,
        tick: u64,
        request: Vec<Message>,
        response: Vec<Message>,
    ) -> ExchangeId {
        let when = at(tick);
        let source = SourceRef::new("fixture.json", format!("/{agent}/{tick}"));
        let id = exchange_id(&dataset(), &source, when);
        let exchange = Exchange {
            id,
            at_us: when,
            client: Client {
                credential: credential.into(),
                session: None,
                turn: None,
                vendor: None,
                model: None,
            },
            request: Request {
                messages: request.iter().map(Message::id).collect(),
                tools: None,
            },
            response: Response {
                messages: response.iter().map(Message::id).collect(),
                stop: None,
                error: None,
            },
            fidelity: Fidelity::Synthetic,
            source,
        };
        for message in request.into_iter().chain(response) {
            self.messages.insert(message.id(), message);
        }
        self.exchanges.push(exchange);
        id
    }

    pub fn decl(&self) -> WorldDecl {
        WorldDecl {
            key: self.key.clone(),
            agents: self.agents.clone(),
        }
    }

    /// The messages and exchanges, as the files hold them.
    pub fn parts(self) -> (WorldDecl, Vec<Message>, Vec<Exchange>) {
        let decl = self.decl();
        (decl, self.messages.into_values().collect(), self.exchanges)
    }

    pub fn finish(self) -> WorldInputs {
        let (decl, messages, exchanges) = self.parts();
        WorldInputs::new(&decl.key.clone(), messages, decl, exchanges)
            .unwrap_or_else(|e| panic!("{e}"))
    }
}

/// One piece of content evidence with its transmission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub transmission: TransmissionRef,
    pub quality: Option<Quality>,
    pub evidence: ContentEvidence,
}

/// Runs the reference with the default config, checks its predictions
/// against the inputs, and flattens its content evidence.
pub fn matched(inputs: &WorldInputs) -> (WorldOutput, Vec<Found>) {
    matched_with(inputs, ReferenceConfig::default())
}

pub fn matched_with(inputs: &WorldInputs, config: ReferenceConfig) -> (WorldOutput, Vec<Found>) {
    let output = run(inputs, config).unwrap_or_else(|e| panic!("{e}"));
    check_predictions(inputs, &output.predictions).unwrap_or_else(|e| panic!("{e}"));
    let found = found(&output);
    (output, found)
}

pub fn found(output: &WorldOutput) -> Vec<Found> {
    output
        .predictions
        .iter()
        .filter_map(|prediction| match prediction {
            Prediction::Transmission(transmission) => Some(transmission.fields()),
            Prediction::Attribution(_) | Prediction::Unattributed(_) => None,
        })
        .flat_map(|fields| {
            fields.matches.iter().map(|evidence| Found {
                transmission: fields.id.clone(),
                quality: fields.quality,
                evidence: evidence.clone(),
            })
        })
        .collect()
}

/// How many transmission rows the output holds.
pub fn transmissions(output: &WorldOutput) -> usize {
    output
        .predictions
        .iter()
        .filter(|prediction| matches!(prediction, Prediction::Transmission(_)))
        .count()
}
