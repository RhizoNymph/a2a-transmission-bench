//! One Claude Code context as model calls with their requests.
//!
//! A call is every assistant entry sharing one `message.id` (the SDK writes
//! one entry per content block). Its request is the context's history when
//! its first block arrived: the compaction summary (if the context began
//! with one), then every earlier call's response and the tool results that
//! followed it. The system prompt and the per-query prompts the SDK sent
//! are not in the table, so requests carry neither (the one recorded
//! prompt string is kept where it occurs).
//!
//! With parallel tool use the SDK interleaves a message's blocks with the
//! results of the tools it already called; those results are held back and
//! placed after the message, where the API saw them.
//!
//! [`ResultRef`] remembers where each tool result sits (its message and
//! part) and which call first carried it: the next call opened after it
//! joined the history, or none when the context ended first.

use std::collections::HashMap;

use a2a_bench_corpus::world::StopReason;
use a2a_bench_format::message::{
    AssistantPart, Body, InvalidMessage, Message, ToolArguments, ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::time::Timestamp;

use super::super::provider::anthropic::stop_reason;
use super::entries::{Entry, EntryKind};

/// One model call of the Claude Code agent.
#[derive(Debug, Clone)]
pub struct Call {
    pub message_id: String,
    /// The row of its first block.
    pub row: String,
    pub at: Timestamp,
    pub model: String,
    pub request: Vec<Message>,
    pub response: Message,
    pub stop: StopReason,
}

/// Where one tool result sits.
#[derive(Debug, Clone)]
pub struct ResultRef {
    pub row: String,
    /// Its block index in the row's content.
    pub block: usize,
    pub at: Timestamp,
    pub message: Message,
    pub part: u16,
    pub call_id: String,
    /// The tool's name, from the call that made it.
    pub tool: String,
    /// Index (in [`Context::calls`]) of the first call carrying it.
    pub reader: Option<usize>,
}

/// One tool call the agent made: which call and the arguments.
#[derive(Debug, Clone)]
pub struct ToolUse {
    pub call: usize,
    pub part: u16,
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub calls: Vec<Call>,
    pub results: Vec<ResultRef>,
    pub tool_uses: Vec<ToolUse>,
}

struct Open {
    message_id: String,
    row: String,
    at: Timestamp,
    model: String,
    parts: Vec<AssistantPart>,
    stop: Option<String>,
    request: Vec<Message>,
}

#[derive(Default)]
struct Walk {
    context: Context,
    history: Vec<Message>,
    open: Option<Open>,
    /// Tool messages that arrived while a call was open, with their refs.
    held: Vec<(Message, Vec<ResultRef>)>,
    /// Refs in the history not yet carried by a call.
    waiting: Vec<usize>,
    names: HashMap<String, String>,
}

impl Walk {
    fn close(&mut self) -> Result<(), InvalidMessage> {
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        let index = self.context.calls.len();
        for (part, assistant) in open.parts.iter().enumerate() {
            if let AssistantPart::ToolCall(call) = assistant {
                self.names.insert(call.call_id.clone(), call.name.clone());
                self.context.tool_uses.push(ToolUse {
                    call: index,
                    part: u16::try_from(part).unwrap_or(u16::MAX),
                    id: call.call_id.clone(),
                    name: call.name.clone(),
                    arguments: match &call.arguments {
                        ToolArguments::Json(json) => json.as_str().to_owned(),
                        ToolArguments::Invalid(raw) => raw.clone(),
                    },
                });
            }
        }
        let stop = stop_reason(open.stop.as_deref(), &open.parts);
        let response = Message::new(Body::Assistant(open.parts))?;
        self.history.push(response.clone());
        self.context.calls.push(Call {
            message_id: open.message_id,
            row: open.row,
            at: open.at,
            model: open.model,
            request: open.request,
            response,
            stop,
        });
        for (message, refs) in std::mem::take(&mut self.held) {
            self.push_results(message, refs);
        }
        Ok(())
    }

    fn push_results(&mut self, message: Message, refs: Vec<ResultRef>) {
        self.history.push(message);
        for mut result in refs {
            if let Some(name) = self.names.get(&result.call_id) {
                result.tool = name.clone();
            }
            self.waiting.push(self.context.results.len());
            self.context.results.push(result);
        }
    }

    fn open(&mut self, entry: &Entry, message_id: &str, model: &str) {
        let index = self.context.calls.len();
        for waiting in std::mem::take(&mut self.waiting) {
            if let Some(result) = self.context.results.get_mut(waiting) {
                result.reader = Some(index);
            }
        }
        self.open = Some(Open {
            message_id: message_id.to_owned(),
            row: entry.row.clone(),
            at: entry.at,
            model: model.to_owned(),
            parts: Vec::new(),
            stop: None,
            request: self.history.clone(),
        });
    }
}

/// The calls and tool results of one context's entries.
pub fn context(entries: &[Entry]) -> Result<Context, InvalidMessage> {
    let mut walk = Walk::default();
    for entry in entries {
        match &entry.kind {
            EntryKind::Assistant {
                message_id,
                model,
                parts,
                stop,
            } => {
                let same = walk
                    .open
                    .as_ref()
                    .is_some_and(|open| &open.message_id == message_id);
                if !same {
                    walk.close()?;
                    walk.open(entry, message_id, model);
                }
                if let Some(open) = walk.open.as_mut() {
                    open.parts.extend(parts.iter().cloned());
                    if stop.is_some() {
                        open.stop.clone_from(stop);
                    }
                }
            }
            EntryKind::ToolResults(results) => {
                let parts: Vec<ToolPart> = results
                    .iter()
                    .map(|(_, result)| ToolPart::ToolResult(result.clone()))
                    .collect();
                if parts.is_empty() {
                    continue;
                }
                let message = Message::new(Body::Tool(parts))?;
                let refs = results
                    .iter()
                    .enumerate()
                    .map(
                        |(part, (block, result)): (usize, &(usize, ToolResult))| ResultRef {
                            row: entry.row.clone(),
                            block: *block,
                            at: entry.at,
                            message: message.clone(),
                            part: u16::try_from(part).unwrap_or(u16::MAX),
                            call_id: result.call_id.clone(),
                            tool: String::new(),
                            reader: None,
                        },
                    )
                    .collect();
                if walk.open.is_some() {
                    walk.held.push((message, refs));
                } else {
                    walk.push_results(message, refs);
                }
            }
            EntryKind::UserText(text) => {
                walk.close()?;
                if !text.is_empty() {
                    walk.history
                        .push(Message::new(Body::User(vec![UserPart::Text {
                            text: text.clone(),
                        }]))?);
                }
            }
            EntryKind::QueryEnd | EntryKind::Boundary => walk.close()?,
            EntryKind::Other => {}
        }
    }
    walk.close()?;
    // Results the context ended on were never carried by a call.
    Ok(walk.context)
}
