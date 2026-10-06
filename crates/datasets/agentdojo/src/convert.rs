//! One run file to one world.

use std::fs;
use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::world::{ExchangeDraft, StopReason, World, WorldAgent, WorldBuilder};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, SourceRef, WorldKey};
use a2a_bench_format::labels::Tier;
use a2a_bench_format::message::{AssistantPart, Body, Message, UserPart};

use crate::schema::Run;
use crate::tally::Tally;
use crate::truth::{Attacker, RunLabels};
use crate::{ATTACKER, AgentDojoError, DATASET, VICTIM, files, messages};

/// Pipeline-name suffixes naming a defense, not a model.
const DEFENSES: &[&str] = &[
    "-repeat_user_prompt",
    "-spotlighting_with_delimiting",
    "-tool_filter",
    "-transformers_pi_detector",
];

/// The attacker agent's declared model.
const ATTACKER_MODEL: &str = "agentdojo-attacker";

/// A converted run and what its conversion counted.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub world: World,
    pub tally: Tally,
}

/// The model behind a pipeline name: the name without a defense suffix.
pub fn model_of(pipeline: &str) -> &str {
    DEFENSES
        .iter()
        .find_map(|suffix| pipeline.strip_suffix(suffix))
        .unwrap_or(pipeline)
}

/// Reads and converts run file `relative` (under `root`), calls `pace`
/// apart.
pub fn load_world(root: &Path, relative: &Path, pace: Pace) -> Result<Loaded, AgentDojoError> {
    let path = root.join(relative);
    let bytes = fs::read(&path).map_err(|source| AgentDojoError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let run: Run = serde_json::from_slice(&bytes).map_err(|source| AgentDojoError::Json {
        path: path.display().to_string(),
        source,
    })?;
    let file = relative.to_string_lossy().replace('\\', "/");
    convert_run(&run, &file, pace)
}

/// Converts a parsed run into a world named after `file` (relative to the
/// dataset root). The attacker's exchange is call step 0 and the victim's
/// response at message `i` step `i + 1`, so each tool run between two of
/// its calls takes a step too.
pub fn convert_run(run: &Run, file: &str, pace: Pace) -> Result<Loaded, AgentDojoError> {
    let dataset = DatasetId::new(DATASET)?;
    let key = WorldKey::new(files::world_name(Path::new(file)))?;
    let mut builder = WorldBuilder::new(dataset, key);
    let model = model_of(&run.pipeline_name);
    let victim = builder.model_agent(VICTIM, model)?;
    let conversation = messages::convert(&run.messages)?;
    let attacker = if run.attacked() {
        let agent = builder.model_agent(ATTACKER, ATTACKER_MODEL)?;
        let exchange = builder.exchange(attacker_draft(run, &agent, file, pace)?)?;
        Some((agent, exchange))
    } else {
        None
    };
    let mut exchanges = Vec::new();
    for (index, raw) in run.messages.iter().enumerate() {
        if raw.role != "assistant" {
            continue;
        }
        let (Some(request), Some(response)) = (
            conversation.messages.get(..index),
            conversation.messages.get(index),
        ) else {
            continue;
        };
        let draft = ExchangeDraft {
            agent: victim.clone(),
            at: pace.at(index as u64 + 1, 0, 0)?,
            model: Some(model.to_owned()),
            request: request.to_vec(),
            response: vec![response.clone()],
            tools: None,
            stop: Some(StopReason::for_calls(!raw.calls().is_empty())),
            session: None,
            turn: None,
            fidelity: Fidelity::Reconstructed,
            source: SourceRef::new(file, format!("/messages/{index}")),
        };
        exchanges.push((index, builder.exchange(draft)?));
    }
    let mut tally = Tally::default();
    let labels = RunLabels {
        file,
        run,
        conversation: &conversation,
        victim: victim.key(),
        attacker: attacker.as_ref().map(|(agent, exchange)| Attacker {
            key: agent.key(),
            exchange: *exchange,
        }),
        exchanges: &exchanges,
    }
    .label(&mut tally)?;
    for label in labels {
        builder.label(label)?;
    }
    tracing::debug!(
        file,
        attack = run.attack_type.as_deref().unwrap_or("none"),
        messages = run.messages.len(),
        "converted AgentDojo run"
    );
    Ok(Loaded {
        world: builder.finish(Coverage::Complete {
            tier: Tier::Construction,
        })?,
        tally,
    })
}

/// The attacker's one exchange: asked for the injections, it writes each
/// non-empty one (in vector order) as one text part.
fn attacker_draft(
    run: &Run,
    attacker: &WorldAgent,
    file: &str,
    pace: Pace,
) -> Result<ExchangeDraft, AgentDojoError> {
    let attack = run.attack_type.as_deref().unwrap_or("unknown");
    let goal = run.injection_task_id.as_deref().unwrap_or("unknown");
    let request = Message::new(Body::User(vec![UserPart::Text {
        text: format!(
            "Write the {attack} injections for {goal} against {} in the {} suite.",
            run.user_task_id, run.suite_name
        ),
    }]))?;
    let parts = run
        .injections()
        .filter(|(_, injection)| !injection.is_empty())
        .map(|(_, injection)| AssistantPart::Text {
            text: injection.clone(),
        })
        .collect();
    Ok(ExchangeDraft {
        agent: attacker.clone(),
        at: pace.at(0, 0, 0)?,
        model: Some(format!("agentdojo-attack/{attack}")),
        request: vec![request],
        response: vec![Message::new(Body::Assistant(parts))?],
        tools: None,
        stop: Some(StopReason::EndTurn),
        session: None,
        turn: None,
        fidelity: Fidelity::Synthetic,
        source: SourceRef::new(file, "/injections"),
    })
}
