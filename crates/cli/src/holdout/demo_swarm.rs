//! demo-swarm's holdout (design §9.2): the unit is a whole run, a fresh
//! node0 run made with a seed of at least [`MIN_SEED`], identified by its
//! truth header's `run` id and its seed.
//!
//! The seed comes from the capture's `bench.env` (in the `--inputs` dir,
//! else its parent): a `seed=<n>` line, else `--seed <n>` in the `swarm=`
//! line (the swarm's arguments, as node0's bench script writes them). With
//! no `bench.env` seed, `--seed` is required. Both given must agree, and
//! the seed must be the truth header's.
//!
//! Runs accumulate, so `splits/demo-swarm@1.holdout.commit` is a list, one
//! line per run, sorted by run id:
//!
//! ```text
//! <run id> <seed> <commitment hex>
//! ```
//!
//! The commitment is the holdout commitment of the run's export
//! ([`super::commitment`]: BLAKE3 derive-key `a2a-bench/1 holdout commit`
//! over the world key, `\n`, `0x00`, the labels digest's hex). A run not
//! yet listed is added; a listed run must have the same seed and
//! commitment, else the export is refused.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_format::ids::Digest;

use super::{Committed, HoldoutError};

/// The smallest seed of a holdout run; smaller seeds are dev runs.
pub const MIN_SEED: u64 = 1_000_000;

/// The run file node0's bench script writes beside a capture.
pub const BENCH_ENV: &str = "bench.env";

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {key} is not a seed")]
    Malformed { path: PathBuf, key: &'static str },
    #[error("{path} names seeds {first} and {second}")]
    Conflicting {
        path: PathBuf,
        first: u64,
        second: u64,
    },
    #[error("no bench.env seed in the inputs dir or its parent: a demo-swarm holdout needs --seed")]
    Required,
    #[error("--seed {flag} differs from the bench.env seed {env}")]
    FlagDiffers { flag: u64, env: u64 },
    #[error("the run's seed is {seed}, its truth header's is {truth}")]
    TruthDiffers { seed: u64, truth: u64 },
    #[error("seed {seed} is below {MIN_SEED}: the run is not a holdout run")]
    NotHoldout { seed: u64 },
}

/// Where a run's seed was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedFrom {
    BenchEnv(PathBuf),
    Flag,
}

/// A holdout run's seed, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSeed {
    pub seed: u64,
    pub from: SeedFrom,
}

/// The seed a `bench.env`'s text names (module docs), if any.
pub fn bench_env_seed(text: &str, path: &Path) -> Result<Option<u64>, SeedError> {
    let malformed = |key| SeedError::Malformed {
        path: path.to_path_buf(),
        key,
    };
    let mut explicit = None;
    let mut swarm = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "seed" => {
                explicit = Some(value.trim().parse::<u64>().map_err(|_| malformed("seed"))?);
            }
            "swarm" => {
                let words = shlex::split(value).ok_or_else(|| malformed("swarm"))?;
                let mut words = words.iter();
                while let Some(word) = words.next() {
                    let text = match word.strip_prefix("--seed") {
                        Some("") => words.next().map(String::as_str),
                        Some(rest) => match rest.strip_prefix('=') {
                            Some(value) => Some(value),
                            None => continue,
                        },
                        None => continue,
                    };
                    let seed = text
                        .and_then(|text| text.parse::<u64>().ok())
                        .ok_or_else(|| malformed("swarm"))?;
                    swarm = Some(seed);
                }
            }
            _ => {}
        }
    }
    match (explicit, swarm) {
        (Some(first), Some(second)) if first != second => Err(SeedError::Conflicting {
            path: path.to_path_buf(),
            first,
            second,
        }),
        (Some(seed), _) | (None, Some(seed)) => Ok(Some(seed)),
        (None, None) => Ok(None),
    }
}

/// The first `bench.env` seed in `inputs` or its parent.
fn env_seed(inputs: &Path) -> Result<Option<(u64, PathBuf)>, SeedError> {
    for dir in [Some(inputs), inputs.parent()].into_iter().flatten() {
        let path = dir.join(BENCH_ENV);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(SeedError::Read { path, source }),
        };
        if let Some(seed) = bench_env_seed(&text, &path)? {
            return Ok(Some((seed, path)));
        }
    }
    Ok(None)
}

/// The holdout run's seed: from `bench.env` beside `inputs` (an absolute
/// dir), else `flag`; equal to `truth` (the truth header's) and at least
/// [`MIN_SEED`].
pub fn run_seed(inputs: &Path, flag: Option<u64>, truth: u64) -> Result<RunSeed, SeedError> {
    let found = match (env_seed(inputs)?, flag) {
        (Some((env, _)), Some(flag)) if env != flag => {
            return Err(SeedError::FlagDiffers { flag, env });
        }
        (Some((seed, path)), _) => RunSeed {
            seed,
            from: SeedFrom::BenchEnv(path),
        },
        (None, Some(seed)) => RunSeed {
            seed,
            from: SeedFrom::Flag,
        },
        (None, None) => return Err(SeedError::Required),
    };
    if found.seed != truth {
        return Err(SeedError::TruthDiffers {
            seed: found.seed,
            truth,
        });
    }
    if found.seed < MIN_SEED {
        return Err(SeedError::NotHoldout { seed: found.seed });
    }
    Ok(found)
}

/// One line of the demo-swarm commitment list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub run: String,
    pub seed: u64,
    pub commitment: Digest,
}

impl Entry {
    /// An entry for run id `run` (non-empty, no whitespace).
    pub fn new(run: &str, seed: u64, commitment: Digest) -> Result<Self, HoldoutError> {
        if run.is_empty() || run.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(HoldoutError::InvalidRun);
        }
        Ok(Self {
            run: run.to_owned(),
            seed,
            commitment,
        })
    }

    fn line(&self) -> String {
        format!("{} {} {}\n", self.run, self.seed, self.commitment.to_hex())
    }
}

fn parse(text: &str, path: &Path) -> Result<BTreeMap<String, Entry>, HoldoutError> {
    let malformed = || HoldoutError::Malformed {
        path: path.to_path_buf(),
    };
    let mut entries = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [run, seed, commitment] = fields.as_slice() else {
            return Err(malformed());
        };
        let seed: u64 = seed.parse().map_err(|_| malformed())?;
        let commitment: Digest = commitment.parse().map_err(|_| malformed())?;
        let entry = Entry::new(run, seed, commitment)?;
        if entries.insert(entry.run.clone(), entry).is_some() {
            return Err(malformed());
        }
    }
    Ok(entries)
}

/// Adds `entry` to the list at `path` (created if missing), or checks it
/// against the run's line there.
pub fn record_or_check(path: &Path, entry: &Entry) -> Result<Committed, HoldoutError> {
    let mut entries = match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(source) => {
            return Err(HoldoutError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if let Some(listed) = entries.get(&entry.run) {
        if listed.seed != entry.seed {
            return Err(HoldoutError::SeedChanged {
                path: path.to_path_buf(),
                run: entry.run.clone(),
                committed: listed.seed,
                actual: entry.seed,
            });
        }
        if listed.commitment != entry.commitment {
            return Err(HoldoutError::Mismatch {
                path: path.to_path_buf(),
                committed: listed.commitment,
                actual: entry.commitment,
            });
        }
        return Ok(Committed::Matched {
            path: path.to_path_buf(),
            commitment: entry.commitment,
        });
    }
    entries.insert(entry.run.clone(), entry.clone());
    let text: String = entries.values().map(Entry::line).collect();
    let write = |source| HoldoutError::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(write)?;
    }
    std::fs::write(path, text).map_err(write)?;
    Ok(Committed::Recorded {
        path: path.to_path_buf(),
        commitment: entry.commitment,
    })
}
