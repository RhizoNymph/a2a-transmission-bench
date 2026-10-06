//! The run: every world of the input view through the matcher, into one
//! predictions file.

use std::fs::File;
use std::io::BufWriter;

use a2a_bench_format::check::check_predictions;
use a2a_bench_format::files::{DetectorInfo, Predictions, PredictionsHeader, PredictionsWorld};
use a2a_bench_format::ids::{Digest, WorldKey};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::jsonl::FileWriter;
use a2a_bench_format::predictions::{Prediction, WorldStatus};
use a2a_bench_reference::{ReferenceConfig, WorldSummary, run as reference_run};
use anyhow::Context;

use crate::args::Args;
use crate::inputs::{WorldFiles, Worlds, manifest};

/// The detector's name in the predictions header; gates select on it.
pub const NAME: &str = "reference";

/// The BLAKE3 derive-key context of the config digest.
pub const CONFIG_DIGEST_CONTEXT: &str = "a2a-bench-reference/1 config";

/// The keyed BLAKE3 of the config's canonical JSON.
fn config_digest(config: &ReferenceConfig) -> anyhow::Result<Digest> {
    let text = serde_json::to_string(config).context("encoding the config")?;
    let canonical = CanonicalJson::canonicalize(&text).context("canonicalizing the config")?;
    Ok(Digest::keyed(
        CONFIG_DIGEST_CONTEXT,
        canonical.as_str().as_bytes(),
    ))
}

pub fn run(args: &Args) -> anyhow::Result<()> {
    let config = args.config();
    let manifest = manifest(&args.input)?;
    let manifest_digest = manifest.digest().context("digesting the manifest")?;
    let mut worlds = Worlds::open(&args.input, &manifest)?;
    let header = PredictionsHeader::new(
        manifest.dataset.clone(),
        DetectorInfo {
            name: NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            variant: format!("max-postings-{}", config.max_postings),
            config_digest: Some(config_digest(&config)?),
        },
        manifest_digest,
    );
    let file = File::create(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;
    let mut writer = FileWriter::<Predictions, _>::new(BufWriter::new(file), &header)
        .context("writing the predictions header")?;
    let (mut scored, mut failed) = (0u64, 0u64);
    while let Some(world) = worlds.next_world()? {
        let key = world.key.clone();
        let (status, rows) = detect(world, config);
        match status {
            WorldStatus::Scored => scored += 1,
            WorldStatus::Failed { .. } | WorldStatus::NoConsumers { .. } => failed += 1,
        }
        writer
            .world(&PredictionsWorld { key, status })
            .context("writing a world row")?;
        for row in &rows {
            writer.row(row).context("writing a prediction")?;
        }
    }
    let (_, trailer) = writer.finish().context("finishing the predictions file")?;
    tracing::info!(
        dataset = %manifest.dataset,
        output = %args.output.display(),
        worlds = trailer.worlds,
        scored,
        failed,
        rows = trailer.rows,
        digest = %trailer.digest,
        "predictions written"
    );
    Ok(())
}

/// One world's status and rows: `scored` with the matcher's predictions,
/// or `failed` with the reason and no rows.
fn detect(world: WorldFiles, config: ReferenceConfig) -> (WorldStatus, Vec<Prediction>) {
    let key = world.key.clone();
    let inputs = match world.into_inputs() {
        Ok(inputs) => inputs,
        Err(error) => return failure(&key, format!("inputs: {error}")),
    };
    let output = match reference_run(&inputs, config) {
        Ok(output) => output,
        Err(error) => return failure(&key, format!("reference: {error}")),
    };
    if let Err(error) = check_predictions(&inputs, &output.predictions) {
        return failure(&key, format!("predictions: {error}"));
    }
    log_summary(&key, &output.summary);
    (WorldStatus::Scored, output.predictions)
}

fn failure(key: &WorldKey, reason: String) -> (WorldStatus, Vec<Prediction>) {
    tracing::warn!(world = %key, reason = %reason, "world failed");
    (WorldStatus::Failed { reason }, Vec::new())
}

fn log_summary(key: &WorldKey, summary: &WorldSummary) {
    tracing::info!(
        world = %key,
        agents = summary.agents,
        exchanges = summary.exchanges,
        spans = summary.spans,
        matches = summary.matches,
        rereads = summary.rereads,
        out_of_reach = summary.out_of_reach,
        transmissions = summary.transmissions,
        "world scored"
    );
}
