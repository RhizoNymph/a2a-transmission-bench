//! One simulation of a results file to one world.

use std::fs;
use std::path::Path;

use a2a_bench_corpus::world::{ExchangeDraft, StopReason, World, WorldAgent, WorldBuilder};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, SourceRef, WorldKey};
use a2a_bench_format::labels::Tier;
use a2a_bench_format::time::Timestamp;

use crate::schema::{RawMessage, Results};
use crate::time::parse_time;
use crate::truth::{Participant, SimulationLabels};
use crate::views::{Side, View, view};
use crate::{AGENT, DATASET, Tau2Error, UNCARRIED_CONTROL, USER, files, prompts};

/// Reads one results file.
pub fn load_results(root: &Path, relative: &Path) -> Result<Results, Tau2Error> {
    let path = root.join(relative);
    let bytes = fs::read(&path).map_err(|source| Tau2Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| Tau2Error::Json {
        path: path.display().to_string(),
        source,
    })
}

/// Converts simulation `index` of a parsed results file named `file`
/// (relative to the dataset root).
pub fn convert_simulation(results: &Results, file: &str, index: usize) -> Result<World, Tau2Error> {
    let simulation = results
        .simulations
        .get(index)
        .ok_or(Tau2Error::NoSimulation(index))?;
    let task = results
        .tasks
        .iter()
        .find(|task| task.id == simulation.task_id)
        .ok_or_else(|| Tau2Error::UnknownTask {
            simulation: index,
            task: simulation.task_id.clone(),
        })?;
    let info = &results.info;
    let messages = &simulation.messages;
    let mut builder = WorldBuilder::new(
        DatasetId::new(DATASET)?,
        WorldKey::new(files::world_name(file, index))?,
    );
    let agent_model = info.agent_info.llm.as_deref().unwrap_or("unknown");
    let user_model = info.user_info.llm.as_deref().unwrap_or("unknown");
    let agent_key = builder.model_agent(AGENT, agent_model)?;
    let has_user = info.user_info.implementation != "dummy_user"
        || messages.iter().any(|message| message.role == "user");
    let user_key = if has_user {
        Some(builder.model_agent(USER, user_model)?)
    } else {
        None
    };
    let (agent_prompt, agent_fidelity) = prompts::agent_system_prompt(info, task);
    let user_prompt = prompts::user_system_prompt(info, task);
    let sim = Sim {
        file,
        index,
        messages,
    };
    let agent = sim.participant(
        &mut builder,
        Side::Agent,
        agent_key,
        view(Side::Agent, messages, agent_prompt.clone())?,
        agent_model,
        agent_fidelity,
    )?;
    let user = match user_key {
        Some(key) => Some(sim.participant(
            &mut builder,
            Side::User,
            key,
            view(Side::User, messages, user_prompt.clone())?,
            user_model,
            Fidelity::Reconstructed,
        )?),
        None => None,
    };
    let labelled = SimulationLabels {
        file,
        simulation: index,
        messages,
        agent: &agent,
        user: user.as_ref(),
        agent_prompt: &agent_prompt,
        user_prompt: &user_prompt,
    }
    .label()?;
    for label in labelled.labels {
        builder.label(label)?;
    }
    builder.add_note(UNCARRIED_CONTROL, labelled.uncarried);
    Ok(builder.finish(Coverage::Complete {
        tier: Tier::Structural,
    })?)
}

/// One simulation's records, for building each side's exchanges.
struct Sim<'a> {
    file: &'a str,
    index: usize,
    messages: &'a [RawMessage],
}

impl Sim<'_> {
    /// Adds `side`'s exchanges (one per model call it made) and returns the
    /// participant.
    fn participant(
        &self,
        builder: &mut WorldBuilder,
        side: Side,
        agent: WorldAgent,
        view: View,
        model: &str,
        fidelity: Fidelity,
    ) -> Result<Participant, Tau2Error> {
        let mut exchanges = Vec::new();
        let mut last: Option<Timestamp> = None;
        for (position, entry) in view.entries.iter().enumerate() {
            let Some(message) = self.messages.get(entry.raw) else {
                continue;
            };
            if message.role != side.role() || !message.is_model_call() {
                continue;
            }
            let recorded = message
                .timestamp
                .as_deref()
                .ok_or(Tau2Error::MissingTime(entry.raw))?;
            let mut at = parse_time(recorded)?;
            // Times are recorded to the microsecond; keep each agent's
            // strictly increasing.
            if let Some(last) = last
                && at <= last
            {
                let next = last
                    .as_micros()
                    .checked_add(1)
                    .ok_or(Tau2Error::TimeOverflow(entry.raw))?;
                at = Timestamp::from_micros(next);
            }
            last = Some(at);
            let draft = ExchangeDraft {
                agent: agent.clone(),
                at,
                model: Some(model.to_owned()),
                request: view.request(position),
                response: vec![entry.message.clone()],
                tools: None,
                stop: Some(stop_reason(message.finish_reason())),
                session: None,
                turn: None,
                fidelity,
                source: SourceRef::new(
                    self.file,
                    format!("/simulations/{}/messages/{}", self.index, entry.raw),
                ),
            };
            exchanges.push((entry.raw, builder.exchange(draft)?));
        }
        Ok(Participant {
            key: agent.key().clone(),
            view,
            exchanges,
        })
    }
}

/// The stop of a provider finish reason.
fn stop_reason(finish: Option<&str>) -> StopReason {
    match finish {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls" | "function_call") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some("content_filter") => StopReason::Refusal,
        _ => StopReason::Other,
    }
}
