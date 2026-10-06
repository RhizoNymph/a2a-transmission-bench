//! OpenAI responses: the Responses API item list and the chat completion
//! message (also what DeepSeek, Kimi, GLM, Grok and other OpenAI-compatible
//! providers return).

/// Responses API items.
///
/// | Item | Bench part |
/// | --- | --- |
/// | `reasoning` | each `summary` / `content` text as `reasoning`; `encrypted_content` as `reasoning_opaque` |
/// | `message` | each `output_text` (or `text`, `refusal`) as `text` |
/// | `function_call` | `tool_call` (`call_id`, `name`, `arguments`) |
/// | `custom_tool_call` | `tool_call` (`call_id`, `name`, `input`) |
/// | `computer_call` | `tool_call` named `computer`, its `action` / `actions` as arguments |
/// | anything else | `unknown` |
pub mod responses {
    use a2a_bench_corpus::world::StopReason;
    use a2a_bench_format::message::{AssistantPart, ToolCall, ToolExecution};
    use serde_json::{Map, Value};

    use super::super::{Response, arguments, opaque, stop_for, text, visible};

    pub fn convert(items: &[Value]) -> Response {
        let mut parts = Vec::new();
        for item in items {
            parts.extend(item_parts(item));
        }
        let stop = stop_for(&parts, StopReason::EndTurn);
        Response { parts, stop }
    }

    fn texts<'a>(item: &'a Value, member: &str) -> impl Iterator<Item = &'a str> {
        item.get(member)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                entry
                    .get("text")
                    .or_else(|| entry.get("refusal"))
                    .and_then(Value::as_str)
            })
    }

    fn call(id: &str, name: &str, args: &Value) -> AssistantPart {
        AssistantPart::ToolCall(ToolCall {
            call_id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments(args),
            execution: ToolExecution::Client,
        })
    }

    fn item_parts(item: &Value) -> Vec<AssistantPart> {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        let string = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or("");
        let id = || {
            let call_id = string("call_id");
            if call_id.is_empty() {
                string("id")
            } else {
                call_id
            }
        };
        match kind {
            "reasoning" => {
                let mut parts: Vec<AssistantPart> = texts(item, "summary")
                    .chain(texts(item, "content"))
                    .filter_map(visible)
                    .collect();
                parts.extend(opaque(string("encrypted_content")));
                parts
            }
            "message" => texts(item, "content").filter_map(text).collect(),
            "function_call" => vec![call(
                id(),
                string("name"),
                item.get("arguments").unwrap_or(&Value::Null),
            )],
            "custom_tool_call" => vec![call(
                id(),
                string("name"),
                item.get("input").unwrap_or(&Value::Null),
            )],
            "computer_call" => {
                let mut args = Map::new();
                for member in ["action", "actions"] {
                    if let Some(value) = item.get(member) {
                        args.insert(member.to_owned(), value.clone());
                    }
                }
                vec![call(id(), "computer", &Value::Object(args))]
            }
            _ => vec![AssistantPart::Unknown],
        }
    }
}

/// A chat completion message: `reasoning_content` or `reasoning` as
/// `reasoning`, `content` as `text`, `tool_calls` as `tool_call`s.
pub mod chat {
    use a2a_bench_corpus::world::StopReason;
    use a2a_bench_format::message::{AssistantPart, ToolCall, ToolExecution};
    use serde_json::Value;

    use super::super::{Response, arguments, stop_for, text, visible};

    pub fn convert(raw: &Value) -> Response {
        let mut parts = Vec::new();
        let reasoning = raw
            .get("reasoning_content")
            .and_then(Value::as_str)
            .or_else(|| raw.get("reasoning").and_then(Value::as_str));
        parts.extend(reasoning.and_then(visible));
        match raw.get("content") {
            Some(Value::String(content)) => parts.extend(text(content)),
            Some(Value::Array(items)) => parts.extend(
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .filter_map(text),
            ),
            _ => {}
        }
        for call in raw
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function").unwrap_or(&Value::Null);
            parts.push(AssistantPart::ToolCall(ToolCall {
                call_id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                arguments: arguments(function.get("arguments").unwrap_or(&Value::Null)),
                execution: ToolExecution::Client,
            }));
        }
        let stop = stop_for(&parts, StopReason::EndTurn);
        Response { parts, stop }
    }
}
