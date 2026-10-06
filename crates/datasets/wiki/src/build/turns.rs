//! Pass 1: every revision as one turn of its author, in the shape a harness
//! gives a conversation.
//!
//! ```text
//! with a read (the page's previous author is someone else):
//!   [.. user "Update P."]                      → GET P            (call)
//!   [.. GET P, result: page body]              → POST P {body}    (read exchange; edit exchange)
//!   [.. POST P, result: "Saved P."]            → "Updated P."
//! without one:
//!   [.. user "Update P."]                      → POST P {body}    (edit exchange)
//!   [.. POST P, result: "Saved P."]            → "Updated P."
//! ```
//!
//! Each agent's request is its previous request and response plus the new
//! inputs. Every exchange is the next call step of the world's pace.

use std::collections::BTreeMap;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::world::{ExchangeDraft, StopReason, WorldAgent, WorldBuilder};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::ids::{AgentKey, ExchangeId, MessageId, SourceRef};
use a2a_bench_format::message::Message;

use super::messages::{assistant_call, assistant_text, system, tool_result, user};
use super::{SYSTEM, inserted_text};
use crate::REVISIONS_FILE;
use crate::error::WikiError;
use crate::resource::page_url;
use crate::schema::Revision;
use crate::tools;

/// What pass 1 recorded for one revision.
pub struct RevRecord {
    pub edit: EditRecord,
    /// The read before the edit, when one was synthesised.
    pub read: Option<ReadRecord>,
}

pub struct EditRecord {
    pub exchange: ExchangeId,
    /// The write (`POST`) response message, for relay locations.
    pub response: MessageId,
}

pub struct ReadRecord {
    /// The edit exchange: its request carries the page body.
    pub exchange: ExchangeId,
    /// The page-body tool-result message, for read locations.
    pub result: MessageId,
}

/// Every agent's conversation so far and the world's call clock.
pub struct Turns {
    pace: Pace,
    /// The next call step.
    step: u64,
    /// Each agent's transcript: its last request and response.
    transcripts: BTreeMap<AgentKey, Vec<Message>>,
}

impl Turns {
    pub fn new(pace: Pace) -> Self {
        Self {
            pace,
            step: 0,
            transcripts: BTreeMap::new(),
        }
    }

    /// One exchange of `agent`: its transcript plus `inputs` as the
    /// request, `response` as the response, at the next call step. The
    /// transcript keeps both, so the agent's next request extends this one.
    fn exchange(
        &mut self,
        builder: &mut WorldBuilder,
        agent: &WorldAgent,
        inputs: Vec<Message>,
        response: Message,
        stop: StopReason,
        source: SourceRef,
    ) -> Result<ExchangeId, WikiError> {
        let at = self.pace.at(self.step, 0, 0)?;
        self.step += 1;
        let transcript = match self.transcripts.entry(agent.key().clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(vec![system(SYSTEM)?])
            }
        };
        transcript.extend(inputs);
        let draft = ExchangeDraft {
            agent: agent.clone(),
            at,
            model: None,
            request: transcript.clone(),
            response: vec![response.clone()],
            tools: None,
            stop: Some(stop),
            session: None,
            turn: None,
            fidelity: Fidelity::Synthetic,
            source,
        };
        let exchange = builder.exchange(draft)?;
        transcript.push(response);
        Ok(exchange)
    }

    /// `rev`'s turn by `actor`: a read of `read`'s body when given, the
    /// edit, and the edit's acknowledgement. `source` is the page-local
    /// source index of each line of `rev`'s body, `own` its own index.
    pub fn revision(
        &mut self,
        builder: &mut WorldBuilder,
        actor: &WorldAgent,
        rev: &Revision,
        read: Option<&Revision>,
        source: &[usize],
        own: usize,
    ) -> Result<RevRecord, WikiError> {
        let url = page_url(&rev.wiki, &rev.name);
        let cite =
            |part: &str| SourceRef::new(REVISIONS_FILE, format!("/rev/{}/{part}", rev.rev_id));
        let task = user(&format!("Update {}.", rev.name))?;

        let inserted = inserted_text(&rev.body, source, own);
        let edit_id = format!("edit-{}", rev.rev_id);
        let edit_call = assistant_call(&edit_id, tools::TOOL, &tools::write_args(&url, &inserted))?;
        let response = edit_call.id();

        let (edit_inputs, read_result) = match read {
            Some(prev) => {
                let read_id = format!("read-{}", rev.rev_id);
                let read_call = assistant_call(&read_id, tools::TOOL, &tools::read_args(&url))?;
                self.exchange(
                    builder,
                    actor,
                    vec![task],
                    read_call,
                    StopReason::ToolUse,
                    cite("read/call"),
                )?;
                let result = tool_result(&read_id, &prev.body)?;
                let result_id = result.id();
                (vec![result], Some(result_id))
            }
            None => (vec![task], None),
        };
        // With a read, the edit exchange's request carries the page body, so
        // it is the read exchange too.
        let edit_path = if read_result.is_some() {
            "read"
        } else {
            "edit"
        };
        let exchange = self.exchange(
            builder,
            actor,
            edit_inputs,
            edit_call,
            StopReason::ToolUse,
            cite(edit_path),
        )?;
        self.exchange(
            builder,
            actor,
            vec![tool_result(&edit_id, &format!("Saved {}.", rev.name))?],
            assistant_text(&format!("Updated {}.", rev.name))?,
            StopReason::EndTurn,
            cite("edit/ack"),
        )?;
        Ok(RevRecord {
            edit: EditRecord { exchange, response },
            read: read_result.map(|result| ReadRecord { exchange, result }),
        })
    }
}
