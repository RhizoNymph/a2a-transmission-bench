//! The synthetic messages of a swarm world.

use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use serde_json::Value;

use crate::error::SwarmError;

/// An assistant message holding one client-executed tool call.
pub fn assistant_call(id: &str, name: &str, args: &Value) -> Result<Message, SwarmError> {
    let json = CanonicalJson::canonicalize(&args.to_string()).map_err(SwarmError::Arguments)?;
    Ok(Message::new(Body::Assistant(vec![
        AssistantPart::ToolCall(ToolCall {
            call_id: id.to_owned(),
            name: name.to_owned(),
            arguments: ToolArguments::Json(json),
            execution: ToolExecution::Client,
        }),
    ]))?)
}

/// A tool message holding one successful text result.
pub fn tool_result(call_id: &str, text: &str) -> Result<Message, SwarmError> {
    Ok(Message::new(Body::Tool(vec![ToolPart::ToolResult(
        ToolResult {
            call_id: call_id.to_owned(),
            content: vec![ResultContent::Text {
                text: text.to_owned(),
            }],
            outcome: ToolOutcome::Success,
        },
    )]))?)
}

pub fn system(text: &str) -> Result<Message, SwarmError> {
    Ok(Message::new(Body::System(vec![SystemPart::Text {
        text: text.to_owned(),
    }]))?)
}

pub fn user(text: &str) -> Result<Message, SwarmError> {
    Ok(Message::new(Body::User(vec![UserPart::Text {
        text: text.to_owned(),
    }]))?)
}

pub fn assistant_text(text: &str) -> Result<Message, SwarmError> {
    Ok(Message::new(Body::Assistant(vec![AssistantPart::Text {
        text: text.to_owned(),
    }]))?)
}
