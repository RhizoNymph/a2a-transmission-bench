//! Shared helpers: small synthetic worlds built by hand. No dataset bytes.

#![allow(dead_code)]

use a2a_bench_corpus::clock;
use a2a_bench_corpus::world::{ExchangeDraft, StopReason, World, WorldAgent, WorldBuilder};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, DatasetId, ExchangeId, LabelId, SourceRef, WorldKey};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::labels::{
    CarrierKind, ExpectedContent, ExpectedTransmission, Label, MatchNeed, Route, Tier,
    TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::time::Timestamp;

pub fn ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|e| panic!("{e}"))
}

pub fn dataset() -> DatasetId {
    ok(DatasetId::new("synthetic"))
}

pub fn world_key(name: &str) -> WorldKey {
    ok(WorldKey::new(name))
}

pub fn agent_key(name: &str) -> AgentKey {
    ok(AgentKey::new(name))
}

pub fn label_id(name: &str) -> LabelId {
    ok(LabelId::new(name))
}

pub fn message(body: Body) -> Message {
    ok(Message::new(body))
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

/// An assistant message making one client tool call with JSON arguments.
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
        outcome: ToolOutcome::Success,
    })]))
}

pub fn tick(n: u64) -> Timestamp {
    ok(clock::ordinal(n))
}

pub fn draft(
    agent: &WorldAgent,
    at: u64,
    request: Vec<Message>,
    response: Message,
) -> ExchangeDraft {
    ExchangeDraft {
        agent: agent.clone(),
        at: tick(at),
        model: None,
        request,
        response: vec![response],
        tools: None,
        stop: Some(StopReason::EndTurn),
        session: None,
        turn: None,
        fidelity: Fidelity::Reconstructed,
        source: SourceRef::new("fixture.json", format!("/{}/{at}", agent.key())),
    }
}

pub const SECRET: &str = "the meeting moved to 4pm";

/// The location of `text` at byte `start` of part 0 of `message` in
/// `exchange`.
pub fn location(exchange: ExchangeId, message: &Message, start: u32, text: &str) -> Location {
    let end = start + ok(u32::try_from(text.len()));
    Location {
        exchange,
        message: message.id(),
        part: 0,
        range: ok(ByteRange::new(start, end)),
    }
}

/// A transmission from `from` (in `sender`) to `to`, read in `reader` at
/// `at`.
pub fn transmission(
    id: &str,
    from: &WorldAgent,
    to: &WorldAgent,
    sender: Option<ExchangeId>,
    reader: ExchangeId,
    at: Location,
    text: &str,
) -> TransmissionFields {
    TransmissionFields {
        id: label_id(id),
        from: from.key().clone(),
        to: to.key().clone(),
        sender_exchange: sender,
        reader_exchange: reader,
        route: Route::Direct,
        carrier: CarrierKind::ToolResult,
        content: ExpectedContent {
            text: text.into(),
            at,
        },
        needs: MatchNeed::Exact,
        tier: Tier::Construction,
        source: SourceRef::new("fixture.json", format!("/labels/{id}")),
    }
}

/// A world: alice sends the secret (a1), bob reads it in a tool result
/// (b1); carol is scripted. `name` is the world key; `secret` varies the
/// content so worlds differ.
pub fn sample_world(name: &str, secret: &str) -> World {
    let mut builder = WorldBuilder::new(dataset(), world_key(name));
    let alice = ok(builder.model_agent("alice", "openai/gpt-4o"));
    let bob = ok(builder.model_agent("bob", "anthropic/claude"));
    ok(builder.scripted_agent("carol"));
    let sent = calls(
        "c1",
        "send",
        &format!(r#"{{"to":"bob","text":"{secret}"}}"#),
    );
    let a1 = ok(builder.exchange(draft(
        &alice,
        1,
        vec![system("You are alice."), user("Tell bob.")],
        sent,
    )));
    let delivered = result("c9", &format!("alice: {secret}"));
    let b1 = ok(builder.exchange(draft(
        &bob,
        2,
        vec![
            system("You are bob."),
            user("Check inbox."),
            calls("c9", "inbox", "{}"),
            delivered.clone(),
        ],
        says("Noted."),
    )));
    let at = location(b1, &delivered, 7, secret);
    ok(
        builder.label(Label::Transmission(ok(ExpectedTransmission::new(
            transmission("t1", &alice, &bob, Some(a1), b1, at, secret),
        )))),
    );
    ok(builder.finish(Coverage::Complete {
        tier: Tier::Construction,
    }))
}

/// Manifest info for the synthetic dataset at source revision `revision`.
pub fn info(revision: &str) -> a2a_bench_corpus::export::ManifestInfo {
    use a2a_bench_format::manifest::{Converter, Setting, Source};
    a2a_bench_corpus::export::ManifestInfo {
        dataset: dataset(),
        dataset_version: 1,
        source: Source {
            path: "synthetic".into(),
            revision: revision.into(),
            digest: a2a_bench_format::ids::Digest::from_bytes([7; 32]),
        },
        converter: Converter {
            version: "0.1.0".into(),
            git: "test".into(),
        },
        selection: [("limit".to_owned(), Setting::Int(3))].into(),
        pace: clock::Pace::DEFAULT.settings(),
    }
}

/// Three sample worlds, w1..w3.
pub fn three_worlds() -> Vec<World> {
    vec![
        sample_world("w1", "first secret"),
        sample_world("w2", "second secret"),
        sample_world("w3", "third secret"),
    ]
}
