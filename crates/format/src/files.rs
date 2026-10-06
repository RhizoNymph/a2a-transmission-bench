//! The four files of an export and a run: messages, exchanges, labels and
//! predictions.

use serde::{Deserialize, Serialize};

use crate::exchange::{Exchange, WorldDecl};
use crate::ids::{DatasetId, Digest, WorldKey};
use crate::jsonl::{BasicHeader, FileKind, HeaderFields, Keyed};
use crate::labels::{Label, Tier};
use crate::message::Message;
use crate::predictions::{Prediction, WorldStatus};
use crate::version::{FORMAT, Format};

/// A world row that carries only its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldOnly {
    pub key: WorldKey,
}

impl Keyed for WorldOnly {
    fn key(&self) -> &WorldKey {
        &self.key
    }
}

impl Keyed for WorldDecl {
    fn key(&self) -> &WorldKey {
        &self.key
    }
}

/// `messages.jsonl`: each world's messages, each once per world.
pub struct Messages;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageRow {
    Message(Message),
}

impl FileKind for Messages {
    const NAME: &'static str = "messages";
    type Header = BasicHeader;
    type World = WorldOnly;
    type Row = MessageRow;
}

/// `exchanges.jsonl`: each world's agents, then its exchanges in time order.
pub struct Exchanges;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExchangeRow {
    Exchange(Exchange),
}

impl FileKind for Exchanges {
    const NAME: &'static str = "exchanges";
    type Header = BasicHeader;
    type World = WorldDecl;
    type Row = ExchangeRow;
}

/// Whether every transmission in a world is labelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Coverage {
    /// Every transmission is labelled, at `tier` or better: an unlabelled
    /// prediction is a false positive.
    Complete { tier: Tier },
    /// Some are not: an unlabelled prediction is unjudged.
    Partial,
}

/// A labels world row: its key and coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelsWorld {
    pub key: WorldKey,
    pub coverage: Coverage,
}

impl Keyed for LabelsWorld {
    fn key(&self) -> &WorldKey {
        &self.key
    }
}

/// `labels.jsonl`: truth. Never handed to a detector.
pub struct Labels;

impl FileKind for Labels {
    const NAME: &'static str = "labels";
    type Header = BasicHeader;
    type World = LabelsWorld;
    type Row = Label;
}

/// Who made a predictions file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorInfo {
    /// Gates select on it: `reference`, `crosstalk-live`, …
    pub name: String,
    /// The detector's own version (a git commit or tag).
    pub version: String,
    /// The setting gates select on beside the name: `forwarding-off`, …
    pub variant: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_digest: Option<Digest>,
}

/// A predictions file's header: which detector, on which export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredictionsHeader {
    pub format: Format,
    pub file: String,
    pub dataset: DatasetId,
    pub detector: DetectorInfo,
    /// The digest of the manifest the detector read.
    pub manifest_digest: Digest,
}

impl PredictionsHeader {
    pub fn new(dataset: DatasetId, detector: DetectorInfo, manifest_digest: Digest) -> Self {
        Self {
            format: FORMAT,
            file: Predictions::NAME.to_owned(),
            dataset,
            detector,
            manifest_digest,
        }
    }
}

impl HeaderFields for PredictionsHeader {
    fn format(&self) -> Format {
        self.format
    }

    fn file(&self) -> &str {
        &self.file
    }

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }
}

/// A predictions world row: its key and how the detector fared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredictionsWorld {
    pub key: WorldKey,
    pub status: WorldStatus,
}

impl Keyed for PredictionsWorld {
    fn key(&self) -> &WorldKey {
        &self.key
    }
}

/// `predictions.jsonl`: what a detector wrote.
pub struct Predictions;

impl FileKind for Predictions {
    const NAME: &'static str = "predictions";
    type Header = PredictionsHeader;
    type World = PredictionsWorld;
    type Row = Prediction;
}
