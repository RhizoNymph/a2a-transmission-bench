//! Detector agents named by what they hold, for `--normalize-ids`.
//!
//! Within a world, every agent with an `attribution` row is renamed to
//! `~<its smallest exchange id>`; exchange sets are disjoint within a
//! world (an exchange is attributed at most once), so the names are unique,
//! and two detectors that split the world's exchanges into agents the same
//! way get the same names whatever they called their agents. The rename is
//! applied everywhere an agent appears: `attribution.agent`,
//! `unattributed.agent`, and `from`/`to` of every match and co-access.
//! Attribution exchanges are sorted. An agent without an attribution row
//! (an `unattributed` one) holds no exchanges and keeps its name.
//!
//! Rows are JSON values: only prediction rows have the `attribution` kind,
//! so the pass leaves rows of other files untouched.

use std::collections::BTreeMap;

use serde_json::Value;

/// The prefix of a canonical agent name.
const PREFIX: &str = "~";

/// Renames the detector agents of one world's rows (module docs).
pub(super) fn canonicalize(rows: &mut [Value]) {
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for row in rows.iter_mut() {
        if row.get("kind").and_then(Value::as_str) != Some("attribution") {
            continue;
        }
        let Some(Value::Array(exchanges)) = row.get_mut("exchanges") else {
            continue;
        };
        exchanges.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        let Some(smallest) = exchanges.first().and_then(Value::as_str) else {
            continue;
        };
        let canonical = format!("{PREFIX}{smallest}");
        if let Some(agent) = row.get("agent").and_then(Value::as_str) {
            names.insert(agent.to_owned(), canonical);
        }
    }
    if names.is_empty() {
        return;
    }
    let rename = |field: Option<&mut Value>| {
        if let Some(field) = field
            && let Some(name) = field.as_str().and_then(|name| names.get(name))
        {
            *field = Value::String(name.clone());
        }
    };
    for row in rows.iter_mut() {
        rename(row.get_mut("agent"));
        for list in ["matches", "co_access"] {
            if let Some(Value::Array(items)) = row.get_mut(list) {
                for item in items {
                    rename(item.get_mut("from"));
                    rename(item.get_mut("to"));
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::canonicalize;

    #[test]
    fn agents_take_their_smallest_exchange_everywhere() {
        let mut rows = vec![
            json!({"kind": "attribution", "agent": "b", "exchanges": ["e3", "e1"]}),
            json!({"kind": "attribution", "agent": "a", "exchanges": ["e2"]}),
            json!({"kind": "unattributed", "agent": "u"}),
            json!({"kind": "transmission", "id": "t", "matches": [{"from": "a", "to": "b"}],
                   "co_access": [{"from": "u", "to": "a"}]}),
        ];
        canonicalize(&mut rows);
        assert_eq!(rows[0]["agent"], "~e1");
        assert_eq!(rows[0]["exchanges"], json!(["e1", "e3"]));
        assert_eq!(rows[1]["agent"], "~e2");
        assert_eq!(rows[2]["agent"], "u");
        assert_eq!(rows[3]["matches"][0], json!({"from": "~e2", "to": "~e1"}));
        assert_eq!(rows[3]["co_access"][0], json!({"from": "u", "to": "~e2"}));
    }

    #[test]
    fn rows_of_other_files_are_untouched() {
        let row = json!({"kind": "exchange_agent", "exchange": "e1", "agent": "a"});
        let mut rows = vec![row.clone()];
        canonicalize(&mut rows);
        assert_eq!(rows, vec![row]);
    }
}
