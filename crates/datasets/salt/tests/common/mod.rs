//! Shared helpers for the SALT tests: the synthetic fixtures, label
//! accessors and a small trace builder.
#![allow(dead_code, clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use a2a_bench_corpus::world::World;
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::ids::AgentKey;
use a2a_bench_format::labels::{ControlFields, Label, NegativeReason, TransmissionFields};
use a2a_bench_format::message::Message;
use serde_json::{Value, json};

pub const MAIN: &str = "traces/main/main__fixture-model/rep001.json";
pub const MEMORY: &str =
    "traces/memory_scope/memory_scope__fixture-model__communication-onward/rep001.json";
pub const CONTROLLED: &str =
    "traces/controlled_peer/controlled_peer__fixture-model__summary/rep001.json";

/// The synthetic dataset root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/salt")
}

pub fn world(file: &str) -> World {
    a2a_bench_dataset_salt::load_world(&root(), Path::new(file))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

pub fn agent(name: &str) -> AgentKey {
    AgentKey::new(name).unwrap_or_else(|e| panic!("{e}"))
}

pub fn positives(world: &World) -> Vec<&TransmissionFields> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(row) => Some(row.fields()),
            _ => None,
        })
        .collect()
}

pub fn negatives(world: &World, reason: NegativeReason) -> Vec<&ControlFields> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::NegativeControl(row) if row.fields().reason == reason => Some(row.fields()),
            _ => None,
        })
        .collect()
}

/// The exchanges of agent `name`, in time order.
pub fn exchanges_of<'w>(world: &'w World, name: &str) -> Vec<&'w Exchange> {
    let key = agent(name);
    world
        .exchanges()
        .iter()
        .filter(|exchange| world.agent_of(exchange.id) == Some(&key))
        .collect()
}

/// The exchange's single response message.
pub fn response<'w>(world: &'w World, exchange: &Exchange) -> &'w Message {
    let id = exchange
        .response
        .messages
        .first()
        .unwrap_or_else(|| panic!("a response"));
    world
        .message(*id)
        .unwrap_or_else(|| panic!("the response message"))
}

/// Whether `exchange`'s request or response names `message`.
pub fn carries(exchange: &Exchange, message: a2a_bench_format::ids::MessageId) -> bool {
    exchange
        .request
        .messages
        .iter()
        .chain(&exchange.response.messages)
        .any(|id| *id == message)
}

/// A minimal SALT trace, built message by message.
#[derive(Debug, Clone, Default)]
pub struct TraceBuilder {
    pub models: Vec<(String, String)>,
    pub episodes: Vec<Value>,
}

impl TraceBuilder {
    pub fn new() -> Self {
        Self {
            models: vec![
                ("alice".into(), "openai/gpt-fixture".into()),
                ("bob".into(), "openai/gpt-fixture".into()),
            ],
            episodes: Vec::new(),
        }
    }

    pub fn value(&self) -> Value {
        let models: serde_json::Map<String, Value> = self
            .models
            .iter()
            .map(|(name, model)| (name.clone(), Value::String(model.clone())))
            .collect();
        json!({
            "condition_id": "fixture",
            "run_config": { "models": models },
            "results": self.episodes,
        })
    }

    pub fn trace(&self) -> a2a_bench_dataset_salt::schema::Trace {
        serde_json::from_value(self.value()).unwrap_or_else(|e| panic!("{e}"))
    }
}

pub fn system(text: &str) -> Value {
    json!({ "role": "system", "content": text })
}

pub fn user(text: &str) -> Value {
    json!({ "role": "user", "content": text })
}

pub fn says(text: &str) -> Value {
    json!({ "role": "assistant", "content": text })
}

/// An assistant message making one call with JSON `arguments`.
pub fn calls(id: &str, name: &str, arguments: &Value) -> Value {
    json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{ "id": id, "type": "function", "function": { "name": name, "arguments": arguments.to_string() } }],
    })
}

pub fn result(id: &str, text: &str) -> Value {
    json!({ "role": "tool", "tool_call_id": id, "content": text })
}

/// The harness's delivered-message turn.
pub fn delivered(from: &str, content: &str) -> Value {
    user(&format!(
        "[round=1/2][from={from}][type=message]\n\n{content}"
    ))
}

pub fn task(episode: u64) -> Value {
    user(&format!("## Episode {episode}: task phase\nDo the task."))
}

pub fn accepted(actor: &str, count: usize) -> Vec<Value> {
    (0..count)
        .map(|_| json!({ "actor": actor, "response_status": "accepted", "finish_reason": "stop" }))
        .collect()
}
