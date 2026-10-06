//! One trace as one world: agents, each episode's exchanges, labels.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::world::{ExchangeDraft, World, WorldAgent, WorldBuilder};
use a2a_bench_format::exchange::{Driven, WorldDecl};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, MessageId, SourceRef, WorldKey};
use a2a_bench_format::labels::Tier;
use a2a_bench_format::message::Message;

use crate::episode::{AgentEpisode, EpisodeClock, episode_steps, reconstruct, stop_reason};
use crate::files::world_name;
use crate::schema::Trace;
use crate::truth::place::{Carriers, place};
use crate::truth::{EpisodeLabels, Labelled};
use crate::{DATASET, SaltError};

/// The model of an agent the run config does not name.
const UNKNOWN_MODEL: &str = "unknown";

/// A declared agent: its handle, whether a model drives it, and the run
/// config's model.
struct Declared {
    agent: WorldAgent,
    driven: Driven,
    model: String,
}

/// Converts a parsed trace into a world named after `file` (relative to the
/// dataset root, `/`-separated), calls `pace` apart.
pub fn convert_trace(trace: &Trace, file: &str, pace: Pace) -> Result<World, SaltError> {
    let dataset = DatasetId::new(DATASET)?;
    let key = WorldKey::new(world_name(Path::new(file)))?;
    let mut builder = WorldBuilder::new(dataset.clone(), key);
    let agents = declare(trace, &mut builder)?;

    let mut carriers = Carriers::default();
    let mut drafts = Vec::new();
    let mut seen_system = BTreeSet::new();
    let mut episode_start = 0u64;
    for (position, episode) in trace.results.iter().enumerate() {
        let clock = EpisodeClock {
            pace,
            start: episode_start,
        };
        let mut reconstructed: Vec<(&Declared, AgentEpisode)> = Vec::new();
        for (name, declared) in &agents {
            let scripted = declared.driven == Driven::Scripted;
            reconstructed.push((declared, reconstruct(name, episode, file, scripted, clock)?));
        }
        let mut labelled = Vec::with_capacity(reconstructed.len());
        for (declared, agent_episode) in &reconstructed {
            let mut exchanges = Vec::new();
            if declared.driven == Driven::Model {
                for turn in &agent_episode.turns {
                    let model = turn
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.requested_model.clone())
                        .unwrap_or_else(|| declared.model.clone());
                    let request = agent_episode
                        .messages
                        .get(..turn.index)
                        .unwrap_or_default()
                        .to_vec();
                    let response: Vec<_> = agent_episode
                        .messages
                        .get(turn.index)
                        .cloned()
                        .into_iter()
                        .collect();
                    let source = SourceRef::new(
                        file,
                        format!(
                            "/results/{position}/agents/{}/messages/{}",
                            declared.agent.key(),
                            turn.index
                        ),
                    );
                    let carried: Vec<MessageId> =
                        request.iter().chain(&response).map(Message::id).collect();
                    let id = builder.exchange(ExchangeDraft {
                        agent: declared.agent.clone(),
                        at: turn.at,
                        model: Some(model),
                        request,
                        response,
                        tools: None,
                        stop: Some(stop_reason(
                            turn.usage.as_ref().and_then(|u| u.finish_reason.as_deref()),
                        )),
                        session: None,
                        turn: None,
                        fidelity: agent_episode.fidelity,
                        source,
                    })?;
                    carriers.add(declared.agent.key(), turn.at, id, carried);
                    exchanges.push(id);
                }
            }
            labelled.push(Labelled {
                key: declared.agent.key().clone(),
                episode: agent_episode,
                exchanges,
                scripted: declared.driven == Driven::Scripted,
            });
        }
        episode_start = episode_start.saturating_add(episode_steps(episode));
        drafts.extend(
            EpisodeLabels {
                file,
                position,
                episode,
                agents: &labelled,
                seen_system: &mut seen_system,
            }
            .label(),
        );
    }
    for label in place(drafts, &carriers)? {
        builder.label(label)?;
    }
    let world = builder.finish(Coverage::Complete {
        tier: Tier::Construction,
    })?;
    tracing::debug!(
        file,
        condition = trace.condition_id.as_deref().unwrap_or(""),
        episodes = trace.results.len(),
        exchanges = world.exchanges().len(),
        "converted SALT trace"
    );
    with_scripted_models(world, &agents)
}

/// Every agent any episode names, in name order: model-driven when one of
/// its calls was accepted, else scripted.
fn declare(
    trace: &Trace,
    builder: &mut WorldBuilder,
) -> Result<BTreeMap<String, Declared>, SaltError> {
    let names: BTreeSet<&str> = trace
        .results
        .iter()
        .flat_map(|episode| episode.agents.keys().map(String::as_str))
        .collect();
    let mut agents = BTreeMap::new();
    for name in names {
        let driven = trace
            .results
            .iter()
            .flat_map(|episode| &episode.llm_usage)
            .any(|usage| usage.actor == name && usage.accepted());
        let model = trace
            .run_config
            .models
            .get(name)
            .map_or(UNKNOWN_MODEL, String::as_str)
            .to_owned();
        let (agent, driven) = if driven {
            (builder.model_agent(name, &model)?, Driven::Model)
        } else {
            (builder.scripted_agent(name)?, Driven::Scripted)
        };
        agents.insert(
            name.to_owned(),
            Declared {
                agent,
                driven,
                model,
            },
        );
    }
    Ok(agents)
}

/// The world with each scripted agent declared with its run-config model,
/// as crosstalk-eval declares it. `WorldBuilder::scripted_agent` takes no
/// model, so the finished world is rebuilt (and checked again) with the
/// declaration amended.
fn with_scripted_models(
    world: World,
    agents: &BTreeMap<String, Declared>,
) -> Result<World, SaltError> {
    let scripted: BTreeMap<_, _> = agents
        .values()
        .filter(|declared| declared.driven == Driven::Scripted)
        .map(|declared| (declared.agent.key().clone(), declared.model.clone()))
        .collect();
    if scripted.is_empty() {
        return Ok(world);
    }
    let mut decl: WorldDecl = world.decl().clone();
    for agent in &mut decl.agents {
        if let Some(model) = scripted.get(&agent.key) {
            agent.model = Some(model.clone());
        }
    }
    let messages = world.messages_in_order().into_iter().cloned().collect();
    Ok(World::new(
        world.dataset().clone(),
        decl,
        messages,
        world.exchanges().to_vec(),
        world.labels().to_vec(),
        world.coverage(),
    )?)
}
