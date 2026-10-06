//! Reading gates files: strict TOML (unknown keys refused), each gate's
//! metric checked against the bounds it takes.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::labels::{CarrierKind, NegativeReason, RouteKind, Tier};

use super::{Check, Gate};
use crate::class::EvidenceClass;

#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("reading gates {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("gates {path} are not valid: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("gates {path}: gate {gate:?}: {problem}")]
    Invalid {
        path: String,
        gate: String,
        problem: InvalidGate,
    },
    #[error("gates file {path} (from --gates) does not exist")]
    Missing { path: String },
}

/// Why a gate's metric and bounds do not fit.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InvalidGate {
    #[error("metric {metric} needs a {bound}")]
    MissingBound {
        metric: &'static str,
        bound: &'static str,
    },
    #[error("metric {metric} takes no {bound}")]
    UnexpectedBound {
        metric: &'static str,
        bound: &'static str,
    },
    #[error("metric {metric} takes no reason")]
    UnexpectedReason { metric: &'static str },
    #[error("a violations max is a whole count, not {value}")]
    NotACount { value: f64 },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    detector: String,
    #[serde(default)]
    gate: Vec<RawGate>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum Metric {
    #[serde(rename = "recall")]
    Recall,
    #[serde(rename = "precision")]
    Precision,
    #[serde(rename = "violations")]
    Violations,
    #[serde(rename = "fp_per_1k")]
    FpPer1k,
}

impl Metric {
    const fn name(self) -> &'static str {
        match self {
            Self::Recall => "recall",
            Self::Precision => "precision",
            Self::Violations => "violations",
            Self::FpPer1k => "fp_per_1k",
        }
    }
}

/// A TOML number: `max = 0` and `max = 130.0` both bound.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
enum Number {
    Int(i64),
    Float(f64),
}

impl Number {
    fn value(self) -> f64 {
        match self {
            // Bounds are small: exact as f64.
            Self::Int(n) => n as f64,
            Self::Float(x) => x,
        }
    }

    fn count(self) -> Result<u64, InvalidGate> {
        match self {
            Self::Int(n) => {
                u64::try_from(n).map_err(|_| InvalidGate::NotACount { value: n as f64 })
            }
            Self::Float(value) => Err(InvalidGate::NotACount { value }),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGate {
    name: String,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    dataset: Option<DatasetId>,
    #[serde(default)]
    route: Option<RouteKind>,
    #[serde(default)]
    carrier: Option<CarrierKind>,
    #[serde(default)]
    class: Option<EvidenceClass>,
    #[serde(default)]
    tier: Option<Tier>,
    metric: Metric,
    #[serde(default)]
    min: Option<Number>,
    #[serde(default)]
    max: Option<Number>,
    #[serde(default)]
    reason: Option<NegativeReason>,
}

impl RawGate {
    fn check(&self) -> Result<Check, InvalidGate> {
        let metric = self.metric.name();
        let need = |bound: Option<Number>, name| {
            bound.ok_or(InvalidGate::MissingBound {
                metric,
                bound: name,
            })
        };
        let none = |bound: Option<Number>, name| match bound {
            Some(_) => Err(InvalidGate::UnexpectedBound {
                metric,
                bound: name,
            }),
            None => Ok(()),
        };
        let no_reason = || match self.reason {
            Some(_) => Err(InvalidGate::UnexpectedReason { metric }),
            None => Ok(()),
        };
        match self.metric {
            Metric::Recall | Metric::Precision => {
                none(self.max, "max")?;
                no_reason()?;
                let min = need(self.min, "min")?.value();
                Ok(match self.metric {
                    Metric::Recall => Check::Recall { min },
                    _ => Check::Precision { min },
                })
            }
            Metric::Violations => {
                none(self.min, "min")?;
                Ok(Check::Violations {
                    max: need(self.max, "max")?.count()?,
                    reason: self.reason,
                })
            }
            Metric::FpPer1k => {
                none(self.min, "min")?;
                no_reason()?;
                Ok(Check::FpPer1k {
                    max: need(self.max, "max")?.value(),
                })
            }
        }
    }
}

pub(super) fn parse(text: &str, path: &str) -> Result<Vec<Gate>, GateError> {
    let file: RawFile = toml::from_str(text).map_err(|source| GateError::Parse {
        path: path.to_owned(),
        source,
    })?;
    file.gate
        .into_iter()
        .map(|raw| {
            let check = raw.check().map_err(|problem| GateError::Invalid {
                path: path.to_owned(),
                gate: raw.name.clone(),
                problem,
            })?;
            Ok(Gate {
                name: raw.name,
                detector: file.detector.clone(),
                variant: raw.variant,
                dataset: raw.dataset,
                route: raw.route,
                carrier: raw.carrier,
                class: raw.class,
                tier: raw.tier,
                check,
            })
        })
        .collect()
}

fn read(path: &Path) -> Result<String, GateError> {
    std::fs::read_to_string(path).map_err(|source| GateError::Read {
        path: path.display().to_string(),
        source,
    })
}

/// The `*.toml` files of `dir`, in name order.
fn toml_files(dir: &Path) -> Result<Vec<PathBuf>, GateError> {
    let entries = std::fs::read_dir(dir).map_err(|source| GateError::Read {
        path: dir.display().to_string(),
        source,
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| GateError::Read {
                path: dir.display().to_string(),
                source,
            })?
            .path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "toml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn load(path: &Path) -> Result<Vec<Gate>, GateError> {
    let files = if path.is_dir() {
        toml_files(path)?
    } else {
        vec![path.to_path_buf()]
    };
    let mut gates = Vec::new();
    for file in files {
        gates.extend(parse(&read(&file)?, &file.display().to_string())?);
    }
    Ok(gates)
}
