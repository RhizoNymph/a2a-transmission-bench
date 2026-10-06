//! The spec table in `docs/features/resource.md`, row by row.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;

use a2a_bench_resource::{
    RemoteError, UrlError, canonical_remote, canonical_url, canonicalize, kind, normalized_url,
};
use common::{parse, render, vectors};

const DOC: &str = include_str!("../../../docs/features/resource.md");

struct Row {
    function: String,
    input: String,
    output: String,
}

/// The rows between the table markers: `| `f` | `input` | `output` |`,
/// with `\|` an escaped `|` and a trailing `†` ignored.
fn rows() -> Vec<Row> {
    let table = DOC
        .split("<!-- spec-table:start -->")
        .nth(1)
        .and_then(|rest| rest.split("<!-- spec-table:end -->").next())
        .expect("the spec table markers");
    table
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<String> = line
                .replace("\\|", "\u{0}")
                .split('|')
                .map(|cell| {
                    cell.trim()
                        .trim_end_matches('†')
                        .trim()
                        .replace('\u{0}', "|")
                })
                .filter(|cell| !cell.is_empty())
                .collect();
            let [function, input, output] = cells.as_slice() else {
                panic!("a row of three cells: {line}");
            };
            let code = |cell: &str| {
                cell.strip_prefix('`')
                    .and_then(|cell| cell.strip_suffix('`'))
                    .unwrap_or_else(|| panic!("a code cell: {cell}"))
                    .to_owned()
            };
            Row {
                function: code(function),
                input: code(input),
                output: code(output),
            }
        })
        .collect()
}

fn url_error(error: &UrlError) -> &'static str {
    match error {
        UrlError::Parse(_) => "parse",
        UrlError::NoHost => "no_host",
    }
}

fn remote_error(error: &RemoteError) -> &'static str {
    match error {
        RemoteError::NotARemote => "not_a_remote",
        RemoteError::Local => "local",
        RemoteError::NoOwner(_) => "no_owner",
        RemoteError::Repository(_) => "repository",
    }
}

fn run(row: &Row) -> String {
    let input = row.input.as_str();
    match row.function.as_str() {
        "canonical_url" => canonical_url(input).map_or_else(
            |error| format!("error {}", url_error(&error)),
            |resource| render(&resource),
        ),
        "normalized_url" => normalized_url(input).map_or_else(
            |error| format!("error {}", url_error(&error)),
            |resource| render(&resource),
        ),
        "canonical_remote" => canonical_remote(input).map_or_else(
            |error| format!("error {}", remote_error(&error)),
            |repo| render(&a2a_bench_format::resource::Resource::Repository(repo)),
        ),
        "canonicalize" => render(&canonicalize(&parse(input))),
        "kind" => kind(&parse(input))
            .map_or("none", |kind| kind.as_str())
            .to_owned(),
        other => panic!("unknown function {other}"),
    }
}

#[test]
fn the_table_has_rows_for_every_function() {
    let functions: BTreeSet<String> = rows().into_iter().map(|row| row.function).collect();
    let expected = [
        "canonical_remote",
        "canonical_url",
        "canonicalize",
        "kind",
        "normalized_url",
    ];
    assert_eq!(
        functions,
        expected.iter().map(|f| (*f).to_owned()).collect()
    );
}

#[test]
fn every_row_holds() {
    let failures: Vec<String> = rows()
        .iter()
        .filter_map(|row| {
            let got = run(row);
            (got != row.output).then(|| {
                format!(
                    "{}({:?}): expected {:?}, got {:?}",
                    row.function, row.input, row.output, got
                )
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_text_row_is_a_parity_vector() {
    let vectors = vectors();
    let inputs: BTreeSet<&str> = vectors["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|vector| vector["input"].as_str().unwrap())
        .collect();
    let missing: Vec<String> = rows()
        .into_iter()
        .filter(|row| {
            matches!(
                row.function.as_str(),
                "canonical_url" | "normalized_url" | "canonical_remote"
            )
        })
        .filter(|row| !inputs.contains(row.input.as_str()))
        .map(|row| row.input)
        .collect();
    assert!(missing.is_empty(), "not in the vectors: {missing:?}");
}

#[test]
fn canonicalize_is_idempotent_on_the_table() {
    for row in rows()
        .iter()
        .filter(|row| matches!(row.function.as_str(), "canonicalize" | "kind"))
    {
        let once = canonicalize(&parse(&row.input));
        assert_eq!(canonicalize(&once), once, "{}", row.input);
    }
}
