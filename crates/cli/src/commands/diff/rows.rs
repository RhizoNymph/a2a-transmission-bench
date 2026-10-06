//! Comparing one file of two runs world by world and row by row.
//!
//! Rows are compared as JSON values (maps ordered, so equal rows have equal
//! text) and identified by `(kind, id)`: a row's `id`, an `exchange_agent`
//! row's `exchange`, an attribution's `agent`. With ids normalised,
//! detector agents are first renamed by their exchanges (`agents`), then a
//! predicted transmission's `id` is dropped and the row is identified by a
//! digest of the rest, so the rows compare as multisets (row order within
//! a world is not compared). Worlds are matched
//! by key; a world only one side holds, and common worlds in another order,
//! are differences too.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use a2a_bench_format::ids::WorldKey;
use a2a_bench_format::jsonl::{FileKind, FileReader, Keyed, WorldSection};
use serde::Serialize;
use serde_json::Value;

use super::{Change, DiffError, Tally, agents};
use crate::safe;

/// How rows are identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Identity {
    /// Name detector agents by their exchanges, drop predicted
    /// transmissions' ids, and ignore row order within a world.
    pub normalize_ids: bool,
}

fn open<K: FileKind>(path: &Path) -> Result<FileReader<K, BufReader<File>>, DiffError> {
    let file = File::open(path).map_err(|source| DiffError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    FileReader::open(BufReader::new(file)).map_err(|error| DiffError::Read {
        path: path.to_path_buf(),
        problem: safe::read_error(&error),
    })
}

fn next<K: FileKind>(
    reader: &mut FileReader<K, BufReader<File>>,
    path: &Path,
) -> Result<Option<WorldSection<K>>, DiffError> {
    reader.next_world().map_err(|error| DiffError::Read {
        path: path.to_path_buf(),
        problem: safe::read_error(&error),
    })
}

fn value<T: Serialize>(row: &T) -> Result<Value, DiffError> {
    serde_json::to_value(row).map_err(DiffError::Encode)
}

fn text(value: &Value) -> Result<String, DiffError> {
    serde_json::to_string(value).map_err(DiffError::Encode)
}

/// A row's `(kind, id)` and its canonical text.
type Row = ((String, String), String);

/// A row's `(kind, id)` and its text.
fn keyed(mut row: Value, identity: Identity) -> Result<Row, DiffError> {
    let kind = row
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("row")
        .to_owned();
    if identity.normalize_ids && kind == "transmission" && row.get("state").is_some() {
        if let Some(map) = row.as_object_mut() {
            map.remove("id");
        }
        let body = text(&row)?;
        let digest = blake3::hash(body.as_bytes()).to_hex();
        return Ok(((kind, format!("~{}", &digest.as_str()[..16])), body));
    }
    let id = ["id", "exchange", "agent"]
        .iter()
        .find_map(|field| row.get(*field).and_then(Value::as_str))
        .unwrap_or("")
        .to_owned();
    Ok(((kind, id), text(&row)?))
}

/// Compares one world's two sections.
fn world<K: FileKind>(
    file: &'static str,
    key: &WorldKey,
    a: &WorldSection<K>,
    b: &WorldSection<K>,
    identity: Identity,
    tally: &mut Tally,
) -> Result<(), DiffError> {
    tally.worlds += 1;
    if value(&a.world)? != value(&b.world)? {
        tally.record(file, Some(key), "world", key.as_str(), Change::Changed);
    }
    let rows = |section: &WorldSection<K>| -> Result<Vec<Row>, DiffError> {
        let mut values = section
            .rows
            .iter()
            .map(value)
            .collect::<Result<Vec<_>, _>>()?;
        if identity.normalize_ids {
            agents::canonicalize(&mut values);
        }
        values.into_iter().map(|row| keyed(row, identity)).collect()
    };
    let (rows_a, rows_b) = (rows(a)?, rows(b)?);
    tally.rows_a += rows_a.len() as u64;
    tally.rows_b += rows_b.len() as u64;
    let group = |rows: &[Row]| {
        let mut map: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for (id, body) in rows {
            map.entry(id.clone()).or_default().push(body.clone());
        }
        map
    };
    let (mut map_a, mut map_b) = (group(&rows_a), group(&rows_b));
    let mut differs = false;
    for (id, bodies_a) in &mut map_a {
        let bodies_b = map_b.remove(id).unwrap_or_default();
        let mut rest_b = bodies_b;
        let mut rest_a = Vec::new();
        for body in bodies_a.drain(..) {
            match rest_b.iter().position(|other| *other == body) {
                Some(at) => {
                    rest_b.swap_remove(at);
                }
                None => rest_a.push(body),
            }
        }
        let paired = rest_a.len().min(rest_b.len());
        for _ in 0..paired {
            differs = true;
            tally.record(file, Some(key), &id.0, &id.1, Change::Changed);
        }
        for _ in paired..rest_a.len() {
            differs = true;
            tally.record(file, Some(key), &id.0, &id.1, Change::OnlyA);
        }
        for _ in paired..rest_b.len() {
            differs = true;
            tally.record(file, Some(key), &id.0, &id.1, Change::OnlyB);
        }
    }
    for (id, bodies_b) in map_b {
        for _ in bodies_b {
            differs = true;
            tally.record(file, Some(key), &id.0, &id.1, Change::OnlyB);
        }
    }
    if !differs && !identity.normalize_ids {
        let order =
            |rows: &[Row]| -> Vec<String> { rows.iter().map(|(_, body)| body.clone()).collect() };
        if order(&rows_a) != order(&rows_b) {
            tally.record(file, Some(key), "row_order", key.as_str(), Change::Order);
        }
    }
    Ok(())
}

/// Compares file `file` of two runs (module docs). Returns the two
/// headers as JSON for the caller to compare.
pub(super) fn file<K: FileKind>(
    file: &'static str,
    path_a: &Path,
    path_b: &Path,
    identity: Identity,
    tally: &mut Tally,
) -> Result<(Value, Value), DiffError> {
    let mut a = open::<K>(path_a)?;
    let mut b = open::<K>(path_b)?;
    let headers = (value(a.header())?, value(b.header())?);
    let mut pending_a: BTreeMap<WorldKey, WorldSection<K>> = BTreeMap::new();
    let mut pending_b: BTreeMap<WorldKey, WorldSection<K>> = BTreeMap::new();
    let (mut order_a, mut order_b) = (Vec::new(), Vec::new());
    loop {
        let (sa, sb) = (next(&mut a, path_a)?, next(&mut b, path_b)?);
        if sa.is_none() && sb.is_none() {
            break;
        }
        if let (Some(x), Some(y)) = (&sa, &sb)
            && x.world.key() == y.world.key()
        {
            order_a.push(x.world.key().clone());
            order_b.push(y.world.key().clone());
            world(file, x.world.key(), x, y, identity, tally)?;
            continue;
        }
        if let Some(x) = sa {
            let key = x.world.key().clone();
            order_a.push(key.clone());
            match pending_b.remove(&key) {
                Some(y) => world(file, &key, &x, &y, identity, tally)?,
                None => {
                    pending_a.insert(key, x);
                }
            }
        }
        if let Some(y) = sb {
            let key = y.world.key().clone();
            order_b.push(key.clone());
            match pending_a.remove(&key) {
                Some(x) => world(file, &key, &x, &y, identity, tally)?,
                None => {
                    pending_b.insert(key, y);
                }
            }
        }
    }
    for key in pending_a.keys() {
        tally.record(file, Some(key), "world", key.as_str(), Change::OnlyA);
    }
    for key in pending_b.keys() {
        tally.record(file, Some(key), "world", key.as_str(), Change::OnlyB);
    }
    let common = |order: &[WorldKey], other: &BTreeMap<WorldKey, WorldSection<K>>| {
        order
            .iter()
            .filter(|key| !other.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>()
    };
    let without_a = common(&order_a, &pending_a);
    let without_b = common(&order_b, &pending_b);
    if without_a != without_b {
        tally.record(file, None, "world_order", "", Change::Order);
    }
    Ok(headers)
}

/// Every JSON-pointer path where `a` and `b` differ (values never shown).
pub(super) fn json_paths(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = format!("{path}/{key}");
                match (x.get(key), y.get(key)) {
                    (Some(x), Some(y)) => json_paths(x, y, &child, out),
                    _ => out.push(child),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            for at in 0..x.len().max(y.len()) {
                let child = format!("{path}/{at}");
                match (x.get(at), y.get(at)) {
                    (Some(x), Some(y)) => json_paths(x, y, &child, out),
                    _ => out.push(child),
                }
            }
        }
        _ if a != b => out.push(if path.is_empty() {
            "/".to_owned()
        } else {
            path.to_owned()
        }),
        _ => {}
    }
}
