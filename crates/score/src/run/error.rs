//! Why a run could not be scored at all (as opposed to one world failing).

use std::fmt;

use serde::{Deserialize, Serialize};

use a2a_bench_format::check::{InputError, LabelError};
use a2a_bench_format::ids::{DatasetId, Digest, WorldKey};
use a2a_bench_format::jsonl::ReadError;

/// One of the four files a run reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileName {
    Messages,
    Exchanges,
    Labels,
    Predictions,
}

impl fmt::Display for FileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Messages => "messages.jsonl",
            Self::Exchanges => "exchanges.jsonl",
            Self::Labels => "labels.jsonl",
            Self::Predictions => "predictions.jsonl",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("opening {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{file}: {source}")]
    Read {
        file: FileName,
        #[source]
        source: Box<ReadError>,
    },
    #[error("{path} is not a manifest: {source}")]
    Manifest {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("the predictions were made on manifest {predictions}, the export's is {export}")]
    ManifestDigest { export: Digest, predictions: Digest },
    #[error("{file} is of dataset {got}, the export is {expected}")]
    DatasetMismatch {
        file: FileName,
        expected: DatasetId,
        got: DatasetId,
    },
    #[error("{file}: world {got} where the export has {expected}")]
    WorldOrder {
        file: FileName,
        expected: WorldKey,
        got: WorldKey,
    },
    #[error("{file} ends before world {expected}")]
    MissingWorld { file: FileName, expected: WorldKey },
    #[error("{file} holds world {got} after the export's last")]
    ExtraWorld { file: FileName, got: WorldKey },
    #[error("world {world}: {source}")]
    Inputs {
        world: WorldKey,
        #[source]
        source: Box<InputError>,
    },
    #[error("world {world}: labels: {source}")]
    Labels {
        world: WorldKey,
        #[source]
        source: Box<LabelError>,
    },
}
