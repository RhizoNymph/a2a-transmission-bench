//! demo-swarm: crosstalk's demo swarm (a traffic generator) run through
//! crosstalk's gateway, labelled from the ground truth the swarm wrote
//! (`truth.jsonl`, version 2). Ported from crosstalk-eval's `swarm_truth`
//! at 7f8a2fb (the labelling half; reading the gateway's own formats stays
//! in crosstalk).
//!
//! This dataset is not converted from raw files. crosstalk's adapter
//! (`ct-bench-detect from-export`) writes a saved run's exchanges as bench
//! `messages.jsonl` and `exchanges.jsonl` (the *capture*), carrying the
//! gateway's minted exchange ids and the harness session and turn in each
//! exchange's `client`. This crate joins the truth's rows to those
//! exchanges and writes the labels, completing an export of
//! `demo-swarm/headline` or `demo-swarm/boilerplate` (the truth header's
//! scenario picks which).
//!
//! ```text
//! truth.jsonl ─▶ truth_file::read ──────────────────┐
//! messages.jsonl + exchanges.jsonl ─▶ capture::read ─┴▶ label::label ─▶ World (checked)
//!                                                         + Diagnostics, counts
//! DemoSwarmSource (TraceSource of that one World) ─▶ corpus export ─▶ export dir
//!   manifest = the capture's manifest.json + labels digest, label counts, notes
//!   (its input view is the capture's, so the capture's predictions score)
//!                                                  + diagnostics.json beside it
//! ```
//!
//! See `docs/features/dataset-demo-swarm.md`.

pub mod capture;
pub mod capture_manifest;
pub mod diagnostics;
pub mod label;
pub mod locate;
pub mod resolve;
pub mod schema;
pub mod sessions;
pub mod source;
pub mod truth_file;
pub mod window;

use std::collections::BTreeMap;

use a2a_bench_format::manifest::Setting;

pub use capture::{Capture, CaptureError};
pub use capture_manifest::{CAPTURE_MANIFEST_FILE, CaptureManifestError};
pub use diagnostics::{Diagnostic, Diagnostics, Effect, JoinFailure, RowKind, Side};
pub use label::{CaptureCounts, DiagnosticsReport, LabelError, Labelled, label};
pub use resolve::{AgentIndex, ResolveCounts};
pub use source::{
    DIAGNOSTICS_FILE, DemoSwarmSource, Error, Inputs, Labelling, TruthRef, write_diagnostics,
    write_export,
};
pub use window::{DEFAULT_LEAD_MS, DEFAULT_SLACK_MS, Margins, RunWindow};

/// The prefix of both dataset ids: a run is exported under
/// `demo-swarm/<scenario>` ([`schema::Scenario::dataset`]).
pub const DATASET_PREFIX: &str = "demo-swarm";

/// The dataset ids this crate exports, by scenario.
pub const HEADLINE: &str = "demo-swarm/headline";
pub const BOILERPLATE: &str = "demo-swarm/boilerplate";

/// The dataset version (`demo-swarm/<scenario>@1`): ct-eval's labels.
pub const VERSION: u32 = 1;

/// The model the world's agents are declared with (the swarm's agents talk
/// to an Anthropic-shaped upstream), as ct-eval declared them.
pub const MODEL: &str = "anthropic/claude";

/// ct-eval's `swarm` selection flags that change the labels: the run
/// window's margins (`--run-lead-ms`, `--run-slack-ms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Before the truth header's start, in milliseconds.
    pub run_lead_ms: u64,
    /// After the truth's latest row time, in milliseconds.
    pub run_slack_ms: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            run_lead_ms: DEFAULT_LEAD_MS,
            run_slack_ms: DEFAULT_SLACK_MS,
        }
    }
}

impl Options {
    pub fn margins(&self) -> Margins {
        Margins {
            lead_ms: self.run_lead_ms,
            slack_ms: self.run_slack_ms,
        }
    }

    /// The manifest's `selection`.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let int = |value: u64| Setting::Int(i64::try_from(value).unwrap_or(i64::MAX));
        BTreeMap::from([
            ("run_lead_ms".to_owned(), int(self.run_lead_ms)),
            ("run_slack_ms".to_owned(), int(self.run_slack_ms)),
        ])
    }
}
