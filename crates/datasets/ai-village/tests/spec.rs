//! The shell model's spec table (`docs/features/dataset-ai-village.md`):
//! every row runs in a fresh shell.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use a2a_bench_dataset_ai_village::shell::outcome::WriteOutcome;
use a2a_bench_dataset_ai_village::shell::{Op, Payload, Shell};

const DOC: &str = include_str!("../../../../docs/features/dataset-ai-village.md");

/// A table cell: code quotes stripped, `\n` a line break, `\|` a pipe.
fn cell(text: &str) -> String {
    let text = text.trim();
    let text = text
        .strip_prefix('`')
        .and_then(|t| t.strip_suffix('`'))
        .unwrap_or(text);
    text.replace("\\n", "\n").replace("\\|", "|")
}

/// Splits a table row on unescaped pipes.
fn cells(row: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = row.trim().trim_start_matches('|').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                current.push_str("\\|");
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    out
}

fn outcome(outcome: WriteOutcome) -> &'static str {
    match outcome {
        WriteOutcome::Delivered => "delivered",
        WriteOutcome::Rejected => "rejected",
        WriteOutcome::Unknown => "unknown",
    }
}

#[test]
fn every_spec_row_holds() {
    let table = DOC
        .split("<!-- spec-table:start -->")
        .nth(1)
        .and_then(|rest| rest.split("<!-- spec-table:end -->").next())
        .expect("the spec table");
    let mut rows = 0;
    for line in table.lines().filter(|l| l.starts_with('|')).skip(2) {
        let columns = cells(line);
        assert_eq!(columns.len(), 4, "{line}");
        let (command, output) = (cell(&columns[0]), cell(&columns[1]));
        let mut shell = Shell::default();
        let found: Vec<String> = shell
            .accesses(&command, &output)
            .iter()
            .map(|access| match &access.op {
                Op::Read => format!("read {}", access.locator),
                Op::Write {
                    outcome: o,
                    payload,
                } => format!(
                    "write {} {} {}",
                    outcome(*o),
                    match payload {
                        Payload::Unseen => "unseen",
                        Payload::Authored(_) => "authored",
                    },
                    access.locator
                ),
            })
            .collect();
        let found = if found.is_empty() {
            "none".to_owned()
        } else {
            found.join("; ")
        };
        assert_eq!(found, cell(&columns[2]), "{command:?}");
        let directory = shell.cwd().unwrap_or("unknown");
        assert_eq!(directory, cell(&columns[3]), "{command:?}");
        rows += 1;
    }
    assert!(rows >= 40, "{rows} rows");
}
