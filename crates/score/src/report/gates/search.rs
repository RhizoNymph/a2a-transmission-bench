//! Where a run's gates come from: `--gates`, then [`GATES_ENV`], then the
//! repository's `gates/` directory, then none. Each place may name one
//! gates file or a directory of them.

use std::ffi::OsString;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{GateError, Gates};

/// The environment variable naming a gates file or directory, after
/// `--gates`.
pub const GATES_ENV: &str = "A2A_BENCH_GATES";

/// The repository's `gates/` directory, which exists in a source checkout.
pub const REPO_GATES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gates");

/// Which place a run's gates came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatesFrom {
    /// `--gates`.
    Flag,
    /// [`GATES_ENV`].
    Env,
    /// [`REPO_GATES`].
    Repo,
}

/// A gates file or directory found, and where from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatesLocation {
    pub from: GatesFrom,
    pub path: PathBuf,
}

/// The places gates are looked for, in order: `flag`, `env`, `repo`. An
/// explicit `flag` must exist; every other place is a default, skipped when
/// missing, and with none found the run has no gates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSearch {
    pub flag: Option<PathBuf>,
    pub env: Option<PathBuf>,
    pub repo: PathBuf,
}

impl GateSearch {
    /// The search for `flag`, the value of [`GATES_ENV`] (`env`; empty is
    /// unset) and `repo`.
    pub fn new(flag: Option<PathBuf>, env: Option<OsString>, repo: PathBuf) -> Self {
        Self {
            flag,
            env: env.filter(|value| !value.is_empty()).map(PathBuf::from),
            repo,
        }
    }

    /// [`GateSearch::new`] with [`GATES_ENV`] read from the process
    /// environment and [`REPO_GATES`].
    pub fn from_env(flag: Option<PathBuf>) -> Self {
        Self::new(flag, std::env::var_os(GATES_ENV), PathBuf::from(REPO_GATES))
    }

    /// The first place found, or `None`; an explicit `flag` that does not
    /// exist is [`GateError::Missing`].
    pub fn locate(&self) -> Result<Option<GatesLocation>, GateError> {
        if let Some(flag) = &self.flag {
            if !flag.exists() {
                return Err(GateError::Missing {
                    path: flag.display().to_string(),
                });
            }
            return Ok(Some(GatesLocation {
                from: GatesFrom::Flag,
                path: flag.clone(),
            }));
        }
        let defaults = [
            (GatesFrom::Env, self.env.as_ref()),
            (GatesFrom::Repo, Some(&self.repo)),
        ];
        Ok(defaults.into_iter().find_map(|(from, path)| {
            path.filter(|path| path.exists()).map(|path| GatesLocation {
                from,
                path: path.clone(),
            })
        }))
    }

    /// The gates of the first place found, and where it was; no gates when
    /// none is.
    pub fn load(&self) -> Result<(Gates, Option<GatesLocation>), GateError> {
        match self.locate()? {
            Some(location) => Ok((Gates::load(&location.path)?, Some(location))),
            None => Ok((Gates::default(), None)),
        }
    }
}
