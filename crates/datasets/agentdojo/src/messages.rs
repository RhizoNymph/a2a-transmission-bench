//! AgentDojo messages as bench messages.
//!
//! | AgentDojo | Bench |
//! | --- | --- |
//! | `system` | `System([Text])` |
//! | `user` | `User([Text])` |
//! | `assistant` | `Assistant`: the text (when not empty), then the tool calls |
//! | `tool` | `Tool([ToolResult])`: the error text and `error` when the call failed (what the pipeline shows the model), else the content and `success` |
//!
//! Many pipelines record calls without ids. A call without one gets
//! `agentdojo-call-<message>-<position>`, and a result without a
//! `tool_call_id` answers the earliest unanswered call equal to its
//! `tool_call` (else the earliest unanswered call).

use std::collections::BTreeMap;

use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use serde_json::Value;

use crate::AgentDojoError;
use crate::schema::{RawCall, RawMessage};

/// A run's messages, converted, with the call each tool message answers.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub messages: Vec<Message>,
    /// Tool message index → the call it answers, when known.
    pub calls: BTreeMap<usize, RawCall>,
}

/// Converts every message of a run.
pub fn convert(raw: &[RawMessage]) -> Result<Conversation, AgentDojoError> {
    let mut messages = Vec::with_capacity(raw.len());
    let mut calls = BTreeMap::new();
    let mut pending: Vec<(String, RawCall)> = Vec::new();
    for (index, message) in raw.iter().enumerate() {
        let text = message.text();
        let body = match message.role.as_str() {
            "system" => Body::System(vec![SystemPart::Text { text }]),
            "user" => Body::User(vec![UserPart::Text { text }]),
            "assistant" => {
                let mut parts = Vec::new();
                if !text.is_empty() {
                    parts.push(AssistantPart::Text { text });
                }
                for (position, call) in message.calls().iter().enumerate() {
                    let id = call
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("agentdojo-call-{index}-{position}"));
                    parts.push(AssistantPart::ToolCall(ToolCall {
                        call_id: id.clone(),
                        name: call.function.clone(),
                        arguments: arguments(&call.args),
                        execution: ToolExecution::Client,
                    }));
                    pending.push((id, call.clone()));
                }
                Body::Assistant(parts)
            }
            "tool" => {
                let (id, call) = answer(&mut pending, message, index);
                if let Some(call) = call {
                    calls.insert(index, call);
                }
                let (text, outcome) = match message.error.as_deref() {
                    Some(error) if !error.is_empty() => (error.to_owned(), ToolOutcome::Error),
                    _ => (text, ToolOutcome::Success),
                };
                Body::Tool(vec![ToolPart::ToolResult(ToolResult {
                    call_id: id,
                    content: vec![ResultContent::Text { text }],
                    outcome,
                })])
            }
            other => return Err(AgentDojoError::UnknownRole(other.to_owned())),
        };
        messages.push(Message::new(body)?);
    }
    Ok(Conversation { messages, calls })
}

/// The id of the call tool message `index` answers, and that call.
fn answer(
    pending: &mut Vec<(String, RawCall)>,
    message: &RawMessage,
    index: usize,
) -> (String, Option<RawCall>) {
    let position = match &message.tool_call_id {
        Some(id) => pending.iter().position(|(pending, _)| pending == id),
        None => message
            .tool_call
            .as_ref()
            .and_then(|call| {
                pending.iter().position(|(_, pending)| {
                    pending.function == call.function && pending.args == call.args
                })
            })
            .or_else(|| (!pending.is_empty()).then_some(0)),
    };
    match position {
        Some(position) => {
            let (id, call) = pending.remove(position);
            (id, message.tool_call.clone().or(Some(call)))
        }
        None => (
            message
                .tool_call_id
                .clone()
                .unwrap_or_else(|| format!("agentdojo-result-{index}")),
            message.tool_call.clone(),
        ),
    }
}

/// Arguments as canonical JSON when they serialize to it, else verbatim.
/// `null` arguments are `{}`.
fn arguments(args: &Value) -> ToolArguments {
    let text = match args {
        Value::Null => "{}".to_owned(),
        other => other.to_string(),
    };
    match CanonicalJson::canonicalize(&text) {
        Ok(json) => ToolArguments::Json(json),
        Err(_) => ToolArguments::Invalid(text),
    }
}
