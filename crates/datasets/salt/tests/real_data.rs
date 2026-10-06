//! The real-data comparison harness (ignored by default: it needs the
//! dataset). It converts a selection of the real SALT dataset and writes a
//! summary with ids, counts and ranges only, never dataset text, for
//! comparison with crosstalk-eval's `truth` output and report:
//!
//! ```text
//! SALT_ROOT=~/Data/ai/agents/salt-nlp SALT_LIMIT=53 SALT_SUMMARY=/path/summary.jsonl \
//!   cargo test -p a2a-bench-dataset-salt --release --test real_data -- --ignored
//! ```
//!
//! One line per world: `{"world", "exchanges", "ids", "labels"}` (label rows
//! as `{"kind", "from", "to", "reader", "sender", "at", "origin", "tier",
//! "route", "carrier", "needs", "reason", "source"}`), or `{"file",
//! "error"}` for a world that failed.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::PathBuf;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_dataset_salt::{Options, source};
use a2a_bench_format::labels::Label;
use a2a_bench_format::location::Location;
use serde_json::{Value, json};

fn place(at: Option<&Location>) -> Value {
    at.map_or(Value::Null, |at| {
        json!({
            "exchange": at.exchange.to_ulid(),
            "part": at.part,
            "start": at.range.start(),
            "end": at.range.end(),
        })
    })
}

fn row(label: &Label) -> Option<Value> {
    Some(match label {
        Label::ExchangeAgent(_) => return None,
        Label::Transmission(row) => {
            let f = row.fields();
            json!({
                "kind": "transmission",
                "id": f.id.as_str(),
                "from": f.from.as_str(),
                "to": f.to.as_str(),
                "reader": f.reader_exchange.to_ulid(),
                "sender": f.sender_exchange.map(|id| id.to_ulid()),
                "at": place(Some(&f.content.at)),
                "text_len": f.content.text.len(),
                "tier": f.tier,
                "route": f.route,
                "carrier": f.carrier,
                "needs": f.needs,
                "source": f.source,
            })
        }
        Label::NegativeControl(row) => {
            let f = row.fields();
            json!({
                "kind": "control",
                "id": f.id.as_str(),
                "from": f.from.as_str(),
                "to": f.to.as_str(),
                "reader": f.reader_exchange.map(|id| id.to_ulid()),
                "at": place(f.at.as_ref()),
                "origin": place(f.origin.as_ref()),
                "has_text": f.text.is_some(),
                "reason": f.reason,
                "tier": f.tier,
                "source": f.source,
            })
        }
        other => json!({ "kind": format!("{other:?}").split('(').next().unwrap_or("other") }),
    })
}

#[test]
#[ignore = "needs the real dataset: set SALT_ROOT, SALT_LIMIT and SALT_SUMMARY"]
fn summarise_a_real_selection() {
    let root = PathBuf::from(std::env::var("SALT_ROOT").expect("SALT_ROOT"));
    let limit = std::env::var("SALT_LIMIT")
        .ok()
        .and_then(|limit| limit.parse().ok());
    let out = PathBuf::from(std::env::var("SALT_SUMMARY").expect("SALT_SUMMARY"));
    let options = Options {
        limit,
        include: vec![],
    };
    let mut salt = source(&root, &options, Pace::DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    let files: Vec<PathBuf> = salt.files().to_vec();
    let mut writer = std::io::BufWriter::new(std::fs::File::create(&out).expect("summary"));
    for (file, world) in files.iter().zip(salt.worlds()) {
        let line = match world {
            Ok(world) => json!({
                "world": world.key().as_str(),
                "exchanges": world.exchanges().len(),
                "ids": world.exchanges().iter().map(|e| e.id.to_ulid()).collect::<Vec<_>>(),
                "fidelity": world.exchanges().iter().map(|e| e.fidelity).collect::<Vec<_>>(),
                "agents": world.decl().agents,
                "labels": world.labels().iter().filter_map(row).collect::<Vec<_>>(),
            }),
            Err(error) => json!({ "file": file, "error": error.to_string() }),
        };
        writeln!(writer, "{line}").expect("write");
    }
    writer.flush().expect("flush");
    let digest = salt.files_read().digest(&root).expect("digest");
    eprintln!(
        "files read: {}, source digest: {digest:?}",
        salt.files_read().len()
    );
}
