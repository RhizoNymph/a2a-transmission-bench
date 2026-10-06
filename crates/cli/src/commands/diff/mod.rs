//! `a2a-bench diff <a> <b> [--normalize-ids] [--first N]`: two exports
//! (directories) or two predictions files, compared structurally world by
//! world and row by row. The tool for parity stages P1–P5.
//!
//! - Exports: the manifests (ignoring `source.digest`) as JSON paths, then
//!   `messages.jsonl`, `exchanges.jsonl` and `labels.jsonl` (when both
//!   hold labels) row by row.
//! - Predictions: the headers as JSON paths, then the rows. With
//!   `--normalize-ids`, predicted transmissions are compared without their
//!   ids (P3: the reference uses its own ids).
//!
//! The report is counts plus the first N differing `(file, world, row
//! kind, id)` tuples; never any text.

mod rows;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{
    EXCHANGES_FILE, ExportError, LABELS_FILE, MANIFEST_FILE, MESSAGES_FILE, read_manifest,
};
use a2a_bench_format::files::{Exchanges, Labels, Messages, Predictions};
use a2a_bench_format::ids::WorldKey;
use clap::Args;
use serde_json::Value;

use rows::{Identity, json_paths};

/// How many differences the report lists by default.
pub const DEFAULT_FIRST: usize = 20;

#[derive(Debug, Clone, Args)]
pub struct DiffArgs {
    /// An export directory or a predictions file.
    pub a: PathBuf,
    /// Of the same kind as `a`.
    pub b: PathBuf,
    /// Predictions: compare transmissions without their ids.
    #[arg(long)]
    pub normalize_ids: bool,
    /// How many differences to list.
    #[arg(long, default_value_t = DEFAULT_FIRST)]
    pub first: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("{a} and {b} are not both export directories or both predictions files")]
    Kinds { a: PathBuf, b: PathBuf },
    #[error(transparent)]
    Manifest(#[from] ExportError),
    #[error("opening {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("reading {path}: {problem}")]
    Read { path: PathBuf, problem: String },
    #[error("encoding a row: {0}")]
    Encode(serde_json::Error),
}

/// How a row differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    OnlyA,
    OnlyB,
    Changed,
    Order,
}

impl Change {
    fn name(self) -> &'static str {
        match self {
            Self::OnlyA => "only in a",
            Self::OnlyB => "only in b",
            Self::Changed => "changed",
            Self::Order => "order",
        }
    }
}

/// One difference: ids only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub file: &'static str,
    pub world: Option<WorldKey>,
    pub kind: String,
    pub id: String,
    pub change: Change,
}

/// Counts and the first differences.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub worlds: u64,
    pub rows_a: u64,
    pub rows_b: u64,
    pub only_a: u64,
    pub only_b: u64,
    pub changed: u64,
    pub order: u64,
    pub first: Vec<Difference>,
    pub cap: usize,
}

impl Tally {
    fn record(
        &mut self,
        file: &'static str,
        world: Option<&WorldKey>,
        kind: &str,
        id: &str,
        change: Change,
    ) {
        match change {
            Change::OnlyA => self.only_a += 1,
            Change::OnlyB => self.only_b += 1,
            Change::Changed => self.changed += 1,
            Change::Order => self.order += 1,
        }
        if self.first.len() < self.cap {
            self.first.push(Difference {
                file,
                world: world.cloned(),
                kind: kind.to_owned(),
                id: id.to_owned(),
                change,
            });
        }
    }

    pub fn differences(&self) -> u64 {
        self.only_a + self.only_b + self.changed + self.order
    }

    pub fn equal(&self) -> bool {
        self.differences() == 0
    }

    /// Counts, then the listed differences.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{} worlds compared, {} rows in a, {} in b: {} differences ({} only in a, {} only in b, {} changed, {} order)",
            self.worlds,
            self.rows_a,
            self.rows_b,
            self.differences(),
            self.only_a,
            self.only_b,
            self.changed,
            self.order
        );
        for difference in &self.first {
            let world = difference
                .world
                .as_ref()
                .map_or_else(|| "-".to_owned(), ToString::to_string);
            let _ = writeln!(
                out,
                "  {:<9} {} world {world} {} {}",
                difference.change.name(),
                difference.file,
                difference.kind,
                difference.id
            );
        }
        if self.differences() > self.first.len() as u64 {
            let _ = writeln!(out, "  …");
        }
        out
    }
}

/// Header or manifest paths that differ, each a difference of `kind`.
fn compare_json(tally: &mut Tally, file: &'static str, kind: &str, a: &Value, b: &Value) {
    let mut paths = Vec::new();
    json_paths(a, b, "", &mut paths);
    for path in paths {
        tally.record(file, None, kind, &path, Change::Changed);
    }
}

fn manifest_value(dir: &Path) -> Result<Value, DiffError> {
    let manifest = read_manifest(dir)?;
    let mut value = serde_json::to_value(&manifest).map_err(DiffError::Encode)?;
    if let Some(source) = value.get_mut("source").and_then(Value::as_object_mut) {
        source.remove("digest");
    }
    Ok(value)
}

/// Compares `args.a` and `args.b` (module docs).
pub fn diff(args: &DiffArgs) -> Result<Tally, DiffError> {
    let mut tally = Tally {
        cap: args.first,
        ..Tally::default()
    };
    let identity = Identity {
        normalize_ids: args.normalize_ids,
    };
    let (a, b) = (&args.a, &args.b);
    let export = |path: &Path| path.join(MANIFEST_FILE).is_file();
    if export(a) && export(b) {
        compare_json(
            &mut tally,
            MANIFEST_FILE,
            "manifest",
            &manifest_value(a)?,
            &manifest_value(b)?,
        );
        let (ha, hb) = rows::file::<Messages>(
            MESSAGES_FILE,
            &a.join(MESSAGES_FILE),
            &b.join(MESSAGES_FILE),
            identity,
            &mut tally,
        )?;
        compare_json(&mut tally, MESSAGES_FILE, "header", &ha, &hb);
        let (ha, hb) = rows::file::<Exchanges>(
            EXCHANGES_FILE,
            &a.join(EXCHANGES_FILE),
            &b.join(EXCHANGES_FILE),
            identity,
            &mut tally,
        )?;
        compare_json(&mut tally, EXCHANGES_FILE, "header", &ha, &hb);
        let (la, lb) = (a.join(LABELS_FILE), b.join(LABELS_FILE));
        match (la.is_file(), lb.is_file()) {
            (true, true) => {
                let (ha, hb) = rows::file::<Labels>(LABELS_FILE, &la, &lb, identity, &mut tally)?;
                compare_json(&mut tally, LABELS_FILE, "header", &ha, &hb);
            }
            (true, false) => tally.record(LABELS_FILE, None, "file", LABELS_FILE, Change::OnlyA),
            (false, true) => tally.record(LABELS_FILE, None, "file", LABELS_FILE, Change::OnlyB),
            (false, false) => {}
        }
    } else if a.is_file() && b.is_file() {
        let (ha, hb) = rows::file::<Predictions>("predictions", a, b, identity, &mut tally)?;
        compare_json(&mut tally, "predictions", "header", &ha, &hb);
    } else {
        return Err(DiffError::Kinds {
            a: a.clone(),
            b: b.clone(),
        });
    }
    Ok(tally)
}
