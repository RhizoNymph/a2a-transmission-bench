//! A small synthetic world shared by the tests: alice posts a page, bob
//! reads it; carol is scripted.

#![allow(dead_code)]

use a2a_bench_format::exchange::{
    AgentDecl, Client, Driven, Exchange, Fidelity, Request, Response, WorldDecl,
};
use a2a_bench_format::ids::{
    AgentKey, DatasetId, ExchangeId, LabelId, SourceRef, WorldKey, exchange_id,
};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::labels::{
    CarrierKind, ExchangeAgent, ExpectedContent, ExpectedTransmission, Label, MatchNeed, Route,
    Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::resource::Resource;
use a2a_bench_format::time::Timestamp;

pub const SECRET: &str = "the meeting moved to 4pm";
pub const PAGE: &str = "https://wiki.example/plan";

pub fn dataset() -> DatasetId {
    DatasetId::new("fixture").unwrap_or_else(|e| panic!("{e}"))
}

pub fn key<T>(
    make: impl Fn(String) -> Result<T, a2a_bench_format::ids::InvalidKey>,
    text: &str,
) -> T {
    make(text.to_owned()).unwrap_or_else(|e| panic!("{e}"))
}

pub fn agent(name: &str) -> AgentKey {
    key(AgentKey::new, name)
}

pub fn label_id(name: &str) -> LabelId {
    key(LabelId::new, name)
}

pub fn world_key() -> WorldKey {
    key(WorldKey::new, "w1")
}

pub fn message(body: Body) -> Message {
    Message::new(body).unwrap_or_else(|e| panic!("{e}"))
}

pub fn system() -> Message {
    message(Body::System(vec![SystemPart::Text {
        text: "You are a helpful agent.".into(),
    }]))
}

pub fn user(text: &str) -> Message {
    message(Body::User(vec![UserPart::Text { text: text.into() }]))
}

pub fn post_call() -> Message {
    let arguments = CanonicalJson::canonicalize(&format!(
        r#"{{"url":"{PAGE}","method":"POST","body":"{SECRET}"}}"#
    ))
    .unwrap_or_else(|e| panic!("{e}"));
    message(Body::Assistant(vec![
        AssistantPart::Text {
            text: "Posting.".into(),
        },
        AssistantPart::ToolCall(ToolCall {
            call_id: "call_1".into(),
            name: "http_request".into(),
            arguments: ToolArguments::Json(arguments),
            execution: ToolExecution::Client,
        }),
    ]))
}

pub fn get_call() -> Message {
    let arguments = CanonicalJson::canonicalize(&format!(r#"{{"url":"{PAGE}","method":"GET"}}"#))
        .unwrap_or_else(|e| panic!("{e}"));
    message(Body::Assistant(vec![AssistantPart::ToolCall(ToolCall {
        call_id: "call_2".into(),
        name: "http_request".into(),
        arguments: ToolArguments::Json(arguments),
        execution: ToolExecution::Client,
    })]))
}

pub fn page_result() -> Message {
    message(Body::Tool(vec![ToolPart::ToolResult(ToolResult {
        call_id: "call_2".into(),
        content: vec![ResultContent::Text {
            text: format!("Plan: {SECRET}."),
        }],
        outcome: ToolOutcome::Unknown,
    })]))
}

pub fn done() -> Message {
    message(Body::Assistant(vec![AssistantPart::Text {
        text: "Noted.".into(),
    }]))
}

fn client(credential: &str) -> Client {
    Client {
        credential: credential.into(),
        session: None,
        turn: None,
        vendor: Some("openai".into()),
        model: None,
    }
}

pub fn at(seconds: u64) -> Timestamp {
    Timestamp::from_micros(1_767_225_600_000_000 + seconds * 1_000_000)
}

fn exchange(
    path: &str,
    when: Timestamp,
    credential: &str,
    request: Vec<&Message>,
    response: Vec<&Message>,
) -> Exchange {
    let source = SourceRef::new("fixture.json", path);
    Exchange {
        id: exchange_id(&dataset(), &source, when),
        at_us: when,
        client: client(credential),
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
    }
}

/// The world: alice posts (a1), bob asks for the page (b1) and reads it (b2).
pub struct Fixture {
    pub decl: WorldDecl,
    pub messages: Vec<Message>,
    pub exchanges: Vec<Exchange>,
}

impl Fixture {
    pub fn a1(&self) -> ExchangeId {
        self.exchanges[0].id
    }

    pub fn b1(&self) -> ExchangeId {
        self.exchanges[1].id
    }

    pub fn b2(&self) -> ExchangeId {
        self.exchanges[2].id
    }

    /// The secret's location in bob's page result (b2's request, message 3, part 0).
    pub fn secret_in_result(&self) -> Location {
        let start = u32::try_from("Plan: ".len()).unwrap_or(0);
        let end = start + u32::try_from(SECRET.len()).unwrap_or(0);
        Location {
            exchange: self.b2(),
            message: page_result().id(),
            part: 0,
            range: ByteRange::new(start, end).unwrap_or_else(|e| panic!("{e}")),
        }
    }

    pub fn labels(&self) -> Vec<Label> {
        vec![
            Label::ExchangeAgent(ExchangeAgent {
                exchange: self.a1(),
                agent: agent("alice"),
            }),
            Label::ExchangeAgent(ExchangeAgent {
                exchange: self.b1(),
                agent: agent("bob"),
            }),
            Label::ExchangeAgent(ExchangeAgent {
                exchange: self.b2(),
                agent: agent("bob"),
            }),
            Label::Transmission(transmission(self)),
        ]
    }
}

pub fn transmission_fields(fixture: &Fixture) -> TransmissionFields {
    TransmissionFields {
        id: label_id("t1"),
        from: agent("alice"),
        to: agent("bob"),
        sender_exchange: Some(fixture.a1()),
        reader_exchange: fixture.b2(),
        route: Route::Channel {
            resource: Resource::Url(PAGE.into()),
        },
        carrier: CarrierKind::ToolResult,
        content: ExpectedContent {
            text: SECRET.into(),
            at: fixture.secret_in_result(),
        },
        needs: MatchNeed::Exact,
        tier: Tier::Heuristic,
        source: SourceRef::new("fixture.json", "/labels/0"),
    }
}

pub fn transmission(fixture: &Fixture) -> ExpectedTransmission {
    ExpectedTransmission::new(transmission_fields(fixture)).unwrap_or_else(|e| panic!("{e}"))
}

pub fn fixture() -> Fixture {
    let (sys, task_a, task_b) = (system(), user("Post the plan."), user("Read the plan."));
    let (post, get, result, ok) = (post_call(), get_call(), page_result(), done());
    let exchanges = vec![
        exchange("/a/0", at(1), "k:alice", vec![&sys, &task_a], vec![&post]),
        exchange("/b/0", at(3), "k:bob", vec![&sys, &task_b], vec![&get]),
        exchange(
            "/b/1",
            at(5),
            "k:bob",
            vec![&sys, &task_b, &get, &result],
            vec![&ok],
        ),
    ];
    let decl = WorldDecl {
        key: world_key(),
        agents: vec![
            AgentDecl {
                key: agent("alice"),
                driven: Driven::Model,
                model: Some("gpt-4o".into()),
            },
            AgentDecl {
                key: agent("bob"),
                driven: Driven::Model,
                model: None,
            },
            AgentDecl {
                key: agent("carol"),
                driven: Driven::Scripted,
                model: None,
            },
        ],
    };
    Fixture {
        decl,
        messages: vec![sys, task_a, task_b, post, get, result, ok],
        exchanges,
    }
}
