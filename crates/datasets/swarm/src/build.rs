//! The two-agent world of one decodable token.
//!
//! - the author originates the decoded plaintext in its output (call 0);
//! - the reader calls `fetch_drop` (call 1) and receives the encoded token
//!   as its result in its next request (call 2), the label's reader
//!   exchange.
//!
//! The label is a `Decoded`-class direct transmission author → reader at
//! the token in the tool result, with the chain the planner verified;
//! Construction tier when the payload has a recovered-text or response
//! child, else Structural. Coverage is complete at Structural.

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::world::{ExchangeDraft, StopReason, World, WorldBuilder};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, LabelId, SourceRef, WorldKey};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedContent, ExpectedTransmission, Label, MatchNeed, Route, Tier,
    TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};

use crate::PAYLOADS_FILE;
use crate::codec::decode;
use crate::error::SwarmError;
use crate::messages::{assistant_call, assistant_text, system, tool_result, user};

/// Every agent's declared model.
pub const MODEL: &str = "swarm/agent";
const SYSTEM: &str = "You are a swarm agent.";

/// One planned world: one token of one payload.
#[derive(Debug, Clone)]
pub struct TokenWorld {
    pub payload_id: String,
    /// The token's index among the payload's candidate tokens.
    pub token_index: usize,
    pub token: String,
    /// The payload has a recovered-text or response child.
    pub corroborated: bool,
}

impl TokenWorld {
    /// `<payload id>#<token index>`.
    pub fn key(&self) -> String {
        format!("{}#{}", self.payload_id, self.token_index)
    }
}

/// Builds the world of `plan`, its calls `pace` apart.
pub fn world(dataset: &DatasetId, plan: &TokenWorld, pace: Pace) -> Result<World, SwarmError> {
    let mut builder = WorldBuilder::new(dataset.clone(), WorldKey::new(plan.key())?);
    let author = builder.model_agent("author", MODEL)?;
    let reader = builder.model_agent("reader", MODEL)?;

    let decoded = decode(&plan.token).ok_or(SwarmError::Undecodable {
        len: plan.token.len(),
    })?;
    let codecs = decoded.codecs().unwrap_or_default();
    let plaintext = decoded.text;
    let row = |path: String| SourceRef::new(PAYLOADS_FILE, path);

    // The author originates the decoded plaintext.
    builder.exchange(ExchangeDraft {
        agent: author.clone(),
        at: pace.at(0, 0, 0)?,
        model: None,
        request: vec![system(SYSTEM)?],
        response: vec![assistant_text(&plaintext)?],
        tools: None,
        stop: Some(StopReason::EndTurn),
        session: None,
        turn: None,
        fidelity: Fidelity::Synthetic,
        source: row(format!("/row/{}/plaintext", plan.payload_id)),
    })?;

    // The reader calls `fetch_drop`, then receives the encoded token as its
    // result in its next request.
    let call_id = format!("fetch-{}", plan.payload_id);
    let call = assistant_call(&call_id, "fetch_drop", &serde_json::json!({}))?;
    let result = tool_result(&call_id, &plan.token)?;
    let result_id = result.id();
    let ask = user("Fetch the drop.")?;
    let token_path = format!("/row/{}/token/{}", plan.payload_id, plan.token_index);
    builder.exchange(ExchangeDraft {
        agent: reader.clone(),
        at: pace.at(1, 0, 0)?,
        model: None,
        request: vec![system(SYSTEM)?, ask.clone()],
        response: vec![call.clone()],
        tools: None,
        stop: Some(StopReason::ToolUse),
        session: None,
        turn: None,
        fidelity: Fidelity::Synthetic,
        source: row(format!("{token_path}/call")),
    })?;
    let reader_exchange = builder.exchange(ExchangeDraft {
        agent: reader.clone(),
        at: pace.at(2, 0, 0)?,
        model: None,
        request: vec![system(SYSTEM)?, ask, call, result],
        response: vec![assistant_text("Fetched the drop.")?],
        tools: None,
        stop: Some(StopReason::EndTurn),
        session: None,
        turn: None,
        fidelity: Fidelity::Synthetic,
        source: row(token_path.clone()),
    })?;

    let end = u32::try_from(plan.token.len()).map_err(|_| SwarmError::TooLong {
        len: plan.token.len(),
    })?;
    let at = Location {
        exchange: reader_exchange,
        message: result_id,
        part: 0,
        range: ByteRange::new(0, end)?,
    };
    let tier = if plan.corroborated {
        Tier::Construction
    } else {
        Tier::Structural
    };
    let label = ExpectedTransmission::new(TransmissionFields {
        id: LabelId::new(token_path.clone())?,
        from: author.key().clone(),
        to: reader.key().clone(),
        sender_exchange: None,
        reader_exchange,
        route: Route::Direct,
        carrier: CarrierKind::ToolResult,
        content: ExpectedContent {
            text: plan.token.clone(),
            at,
        },
        needs: MatchNeed::Decoded { codecs },
        tier,
        source: row(token_path),
    })?;
    builder.label(Label::Transmission(label))?;

    Ok(builder.finish(Coverage::Complete {
        tier: Tier::Structural,
    })?)
}
