//! The pool of trajectories splices are drawn from, and what the generator
//! reads off a trajectory: its working directory, its writes, where a read
//! can go.

use a2a_bench_corpus::helpers::chat::ChatMessage;
use a2a_bench_dataset_open_swe::{OpenSweRow, Shard};

use crate::path::normalize_path;
use crate::write::{self, FileWrite};
use crate::{DEFAULT_WORKDIR, MAX_CONTENT, MIN_CONTENT};

/// A trajectory of the pool.
#[derive(Debug, Clone)]
pub struct Pooled {
    pub shard: Shard,
    pub row: usize,
    pub record: OpenSweRow,
    pub workdir: String,
}

impl Pooled {
    pub fn new(shard: Shard, row: usize, record: OpenSweRow) -> Self {
        let workdir = workdir(&record.messages);
        Self {
            shard,
            row,
            record,
            workdir,
        }
    }

    /// Writes worth splicing.
    pub fn writes(&self) -> Vec<FileWrite> {
        write::writes(&self.record.messages, &self.workdir)
            .into_iter()
            .filter(splicable)
            .collect()
    }
}

/// Whether a write is long enough to carry a span and short enough to view.
pub fn splicable(write: &FileWrite) -> bool {
    (MIN_CONTENT..=MAX_CONTENT).contains(&write.content.len()) && write.content.lines().count() >= 3
}

/// The working directory a trajectory's task names in
/// `<uploaded_files>…</uploaded_files>`, else [`DEFAULT_WORKDIR`].
pub fn workdir(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .filter(|message| message.role == "user")
        .find_map(|message| {
            let text = message.text();
            let start = text.find("<uploaded_files>")? + "<uploaded_files>".len();
            let end = text.get(start..)?.find("</uploaded_files>")? + start;
            let dir = text.get(start..end)?.trim();
            (dir.starts_with('/') && !dir.contains(char::is_whitespace))
                .then(|| normalize_path(dir))
        })
        .unwrap_or_else(|| DEFAULT_WORKDIR.to_owned())
}

/// `messages` with every occurrence of directory `from` replaced by `to`.
pub fn rewrite_workdir(messages: &[ChatMessage], from: &str, to: &str) -> Vec<ChatMessage> {
    if from == to {
        return messages.to_vec();
    }
    let swap = |text: &Option<String>| text.as_ref().map(|text| text.replace(from, to));
    messages
        .iter()
        .map(|message| ChatMessage {
            role: message.role.clone(),
            content: swap(&message.content),
            reasoning_content: swap(&message.reasoning_content),
            tool_calls: message.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|call| {
                        let mut call = call.clone();
                        call.function.arguments = swap(&call.function.arguments);
                        call
                    })
                    .collect()
            }),
            tool_call_id: message.tool_call_id.clone(),
        })
        .collect()
}

/// Message indices before which a read can be spliced: an assistant
/// message that is not the trajectory's first and follows a non-assistant
/// message.
pub fn insertion_points(messages: &[ChatMessage]) -> Vec<usize> {
    let first = messages.iter().position(ChatMessage::is_assistant);
    messages
        .windows(2)
        .enumerate()
        .filter_map(|(before, pair)| match pair {
            [previous, message]
                if message.is_assistant()
                    && Some(before + 1) != first
                    && !previous.is_assistant() =>
            {
                Some(before + 1)
            }
            _ => None,
        })
        .collect()
}

/// How many assistant messages (calls) come before message `index`.
pub fn calls_before(messages: &[ChatMessage], index: usize) -> u64 {
    messages
        .iter()
        .take(index)
        .filter(|message| message.is_assistant())
        .count() as u64
}
