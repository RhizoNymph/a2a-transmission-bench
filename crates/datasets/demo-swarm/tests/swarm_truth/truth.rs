//! The truth file: strict v2 decoding, the header, row order and worlds.

use std::io::Cursor;

use a2a_bench_dataset_demo_swarm::schema::{HexDigest, TruthCarrier, TruthLine, TruthRoute};
use a2a_bench_dataset_demo_swarm::truth_file::{self, DeliveryKind, Row, TruthFileError};
use serde_json::json;

use super::fixture::{self, P1};
use super::read_rows;

#[test]
fn a_v2_file_reads_every_kind_in_order() {
    let mut text = String::new();
    for row in fixture::truth_rows() {
        text.push_str(&row.to_string());
        text.push('\n');
    }
    let truth = truth_file::read(Cursor::new(text)).expect("a valid truth file");
    assert_eq!(truth.header.world, fixture::WORLD);
    assert_eq!(truth.header.version, 2);
    let kinds: Vec<&str> = truth
        .rows
        .iter()
        .map(|numbered| match &numbered.row {
            Row::Delivery { kind, .. } => match kind {
                DeliveryKind::Transmission => "transmission",
                DeliveryKind::SelfRead => "self_read",
                DeliveryKind::Reread => "reread",
            },
            Row::Miss(_) => "miss",
            Row::Unattributed(_) => "unattributed_read",
            Row::Cluster(_) => "agent_cluster",
            Row::Session(_) => "session",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "transmission",
            "transmission",
            "self_read",
            "reread",
            "miss",
            "transmission",
            "transmission",
            "agent_cluster"
        ]
    );
    assert_eq!(truth.rows[0].line, 2);
}

#[test]
fn blank_lines_are_skipped_and_keep_line_numbers() {
    let rows = fixture::truth_rows();
    let text = format!("{}\n\n{}\n", rows[0], rows[1]);
    let truth = truth_file::read(Cursor::new(text)).expect("a valid truth file");
    assert_eq!(truth.rows.len(), 1);
    assert_eq!(truth.rows[0].line, 3);
}

#[test]
fn another_version_is_refused() {
    let mut header = fixture::header();
    header["version"] = json!(1);
    assert!(matches!(
        read_rows(&[header]),
        Err(TruthFileError::UnsupportedVersion { found: 1 })
    ));
}

#[test]
fn an_unknown_kind_or_field_is_refused() {
    let mut row = fixture::truth_rows()[1].clone();
    row["kind"] = json!("delegation");
    assert!(matches!(
        read_rows(&[fixture::header(), row]),
        Err(TruthFileError::Decode { line: 2, .. })
    ));
    let mut row = fixture::truth_rows()[1].clone();
    row["extra"] = json!(true);
    assert!(matches!(
        read_rows(&[fixture::header(), row]),
        Err(TruthFileError::Decode { line: 2, .. })
    ));
}

#[test]
fn a_missing_field_is_refused() {
    let mut row = fixture::truth_rows()[1].clone();
    row.as_object_mut().unwrap().remove("reader_turn");
    assert!(matches!(
        read_rows(&[fixture::header(), row]),
        Err(TruthFileError::Decode { line: 2, .. })
    ));
}

#[test]
fn a_bad_digest_is_refused() {
    let mut row = fixture::truth_rows()[1].clone();
    row["content"]["blake3"] = json!("xyz");
    assert!(matches!(
        read_rows(&[fixture::header(), row]),
        Err(TruthFileError::Decode { line: 2, .. })
    ));
    let digest = HexDigest::blake3_of(b"abc");
    assert_eq!(HexDigest::parse(&digest.to_string()), Ok(digest));
    assert_eq!(
        HexDigest::parse(&digest.to_string().to_uppercase()),
        Ok(digest)
    );
}

#[test]
fn the_header_comes_first_once_and_rows_share_its_world() {
    let rows = fixture::truth_rows();
    assert!(matches!(
        read_rows(&[rows[1].clone()]),
        Err(TruthFileError::HeaderNotFirst { line: 1 })
    ));
    assert!(matches!(
        read_rows(&[fixture::header(), fixture::header()]),
        Err(TruthFileError::SecondHeader { line: 2 })
    ));
    assert!(matches!(read_rows(&[]), Err(TruthFileError::NoHeader)));
    let mut row = rows[1].clone();
    row["world"] = json!("swarm-other");
    assert!(matches!(
        read_rows(&[fixture::header(), row]),
        Err(TruthFileError::OtherWorld { line: 2, .. })
    ));
}

/// Lines from a real local swarm run (synthetic swarm output), verbatim,
/// as ct-eval's test pinned them.
const SAMPLE: &str = r#"{"kind":"header","version":2,"world":"swarm-01M45MWNEKJ1H4M3F2A2QECGDQ","run":"01M45MWNEKJ1H4M3F2A2QECGDQ","seed":7,"agents":4,"keys":2,"agents_per_key":3,"claude_code_shape":false,"started_at_unix_ms":1791191045587,"gateway_url":"http://127.0.0.1:18070","wiki_url":"http://127.0.0.1:18091"}
{"kind":"agent_cluster","world":"swarm-01M45MWNEKJ1H4M3F2A2QECGDQ","key_group":0,"agents":["agent-000","agent-001","agent-002"]}
{"kind":"transmission","world":"swarm-01M45MWNEKJ1H4M3F2A2QECGDQ","writer":"agent-000","reader":"agent-002","page":"rate-limiting-0","version":2,"writer_key_group":0,"reader_key_group":0,"writer_session":"9c0870f8-0dc3-49b1-8a44-2f50eccb87fd","writer_turn":0,"writer_tool_use_id":"toolu_01udn93Yxd0Sio7QLxy2g6vM","reader_session":"7367f467-b421-4961-848f-15dd3e5a1061","reader_turn":1,"reader_tool_use_id":"toolu_01VF5dXLfUFzgmMPG8AaCWKl","route":{"kind":"channel","url":"http://127.0.0.1:18091/pages/rate-limiting-0"},"carrier":"tool_result","read_tool":{"name":"http_request","input":{"method":"GET","url":"http://127.0.0.1:18091/pages/rate-limiting-0"}},"content":{"blake3":"3a8e56e9893d2d4abfcc57cd8bfb0394702a760ec24c998dfe3b33a3a1081873","sha256":"8f1953d2eb28b723a1e75bbc807e3cb6747de2f93d93ce49897ab6c5e4196803","excerpt":"revisit per-tenant quota after the next release. We measured leaky bucket on the","at":{"message":2,"block":0,"tool_use_id":"toolu_01VF5dXLfUFzgmMPG8AaCWKl"}},"at_ms":308,"at_unix_ms":1791191045895,"written_at_unix_ms":1791191045859,"read_at_unix_ms":1791191045895}
"#;

#[test]
fn a_real_swarm_run_parses_exactly() {
    let truth = truth_file::read(Cursor::new(SAMPLE)).expect("the sample parses");
    let header = &truth.header;
    assert_eq!(header.version, 2);
    assert_eq!(header.world, "swarm-01M45MWNEKJ1H4M3F2A2QECGDQ");
    assert_eq!(header.run, "01M45MWNEKJ1H4M3F2A2QECGDQ");
    assert_eq!(
        (
            header.seed,
            header.agents,
            header.keys,
            header.agents_per_key
        ),
        (7, 4, 2, 3)
    );
    assert!(!header.claude_code_shape);
    assert_eq!(header.started_at_unix_ms, 1_791_191_045_587);
    assert_eq!(header.gateway_url, "http://127.0.0.1:18070");
    assert_eq!(header.wiki_url, "http://127.0.0.1:18091");
    assert_eq!(truth.rows.len(), 2);
    let Row::Cluster(cluster) = &truth.rows[0].row else {
        panic!("line 2 is a key group");
    };
    assert_eq!(cluster.key_group, 0);
    assert_eq!(cluster.agents, ["agent-000", "agent-001", "agent-002"]);
    let Row::Delivery { kind, row } = &truth.rows[1].row else {
        panic!("line 3 is a delivery");
    };
    assert_eq!(*kind, DeliveryKind::Transmission);
    assert_eq!(truth.rows[1].line, 3);
    assert_eq!(
        (row.writer.as_str(), row.reader.as_str()),
        ("agent-000", "agent-002")
    );
    assert_eq!((row.page.as_str(), row.version), ("rate-limiting-0", 2));
    assert_eq!((row.writer_key_group, row.reader_key_group), (0, 0));
    assert_eq!(row.writer_session, "9c0870f8-0dc3-49b1-8a44-2f50eccb87fd");
    assert_eq!(
        (row.writer_turn, row.writer_tool_use_id.as_str()),
        (0, "toolu_01udn93Yxd0Sio7QLxy2g6vM")
    );
    assert_eq!(row.reader_session, "7367f467-b421-4961-848f-15dd3e5a1061");
    assert_eq!(
        (row.reader_turn, row.reader_tool_use_id.as_str()),
        (1, "toolu_01VF5dXLfUFzgmMPG8AaCWKl")
    );
    assert_eq!(
        row.route,
        TruthRoute::Channel {
            url: "http://127.0.0.1:18091/pages/rate-limiting-0".to_owned()
        }
    );
    assert_eq!(row.carrier, TruthCarrier::ToolResult);
    assert_eq!(row.read_tool.name, "http_request");
    assert_eq!(
        row.read_tool.input,
        json!({"method": "GET", "url": "http://127.0.0.1:18091/pages/rate-limiting-0"})
    );
    assert_eq!(
        row.content.blake3.to_string(),
        "3a8e56e9893d2d4abfcc57cd8bfb0394702a760ec24c998dfe3b33a3a1081873"
    );
    assert_eq!(
        row.content.sha256.to_string(),
        "8f1953d2eb28b723a1e75bbc807e3cb6747de2f93d93ce49897ab6c5e4196803"
    );
    assert_eq!(
        row.content.excerpt,
        "revisit per-tenant quota after the next release. We measured leaky bucket on the"
    );
    assert_eq!(
        (
            row.content.at.message,
            row.content.at.block,
            row.content.at.tool_use_id.as_str()
        ),
        (2, 0, "toolu_01VF5dXLfUFzgmMPG8AaCWKl")
    );
    assert_eq!(row.at_ms, 308);
    assert_eq!(row.at_unix_ms, header.started_at_unix_ms + row.at_ms);
    assert_eq!(row.written_at_unix_ms, 1_791_191_045_859);
    assert_eq!(row.read_at_unix_ms, 1_791_191_045_895);
    // Re-encoding gives back the same values, key for key.
    for (line, original) in SAMPLE.lines().enumerate() {
        let parsed: TruthLine = serde_json::from_str(original).expect("a line");
        let again: serde_json::Value = serde_json::to_value(&parsed).expect("encode");
        let original: serde_json::Value = serde_json::from_str(original).expect("json");
        assert_eq!(again, original, "line {}", line + 1);
    }
}

fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    names.sort();
    names
}

/// The key sets crates/demo pins for `self_read`, `reread` and `miss`
/// decode.
#[test]
fn every_v2_kind_decodes_with_the_pinned_keys() {
    let rows = fixture::truth_rows();
    let self_read = rows[3].clone();
    let reread = rows[4].clone();
    let miss = rows[5].clone();
    let delivered = [
        "kind",
        "world",
        "writer",
        "reader",
        "page",
        "version",
        "writer_key_group",
        "reader_key_group",
        "writer_session",
        "writer_turn",
        "writer_tool_use_id",
        "reader_session",
        "reader_turn",
        "reader_tool_use_id",
        "route",
        "carrier",
        "read_tool",
        "content",
        "at_ms",
        "at_unix_ms",
        "written_at_unix_ms",
        "read_at_unix_ms",
    ];
    let miss_keys = [
        "kind",
        "world",
        "reader",
        "reader_key_group",
        "page",
        "reader_session",
        "reader_turn",
        "reader_tool_use_id",
        "read_tool",
        "at_ms",
        "at_unix_ms",
    ];
    assert_eq!(sorted_keys(&self_read), sorted(&delivered));
    assert_eq!(sorted_keys(&reread), sorted(&delivered));
    assert_eq!(sorted_keys(&miss), sorted(&miss_keys));
    let truth = read_rows(&[fixture::header(), self_read, reread, miss]).expect("decodes");
    assert_eq!(truth.rows.len(), 3);
}

#[test]
fn an_unattributed_read_decodes_with_the_pinned_keys() {
    let row = fixture::unattributed(("a002", "session-a002", 1, "toolu_r1"), "p1", P1);
    let pinned = [
        "kind",
        "world",
        "reader",
        "reader_key_group",
        "page",
        "version",
        "reader_session",
        "reader_turn",
        "reader_tool_use_id",
        "read_tool",
        "content",
        "at_ms",
        "at_unix_ms",
    ];
    assert_eq!(sorted_keys(&row), sorted(&pinned));
    let truth = read_rows(&[fixture::header(), row]).expect("decodes");
    let Row::Unattributed(read) = &truth.rows[0].row else {
        panic!("an unattributed read");
    };
    assert_eq!((read.reader.as_str(), read.version), ("a002", 5));
    let mut extra = fixture::unattributed(("a002", "session-a002", 1, "toolu_r1"), "p1", P1);
    extra["writer"] = json!("a001");
    assert!(matches!(
        read_rows(&[fixture::header(), extra]),
        Err(TruthFileError::Decode { line: 2, .. })
    ));
}

#[test]
fn a_session_row_decodes_with_exactly_the_pinned_keys() {
    let row = fixture::session("a002", "session-a002");
    let pinned = [
        "kind",
        "world",
        "agent",
        "key_group",
        "session",
        "started_at_unix_ms",
    ];
    assert_eq!(sorted_keys(&row), sorted(&pinned));
    let truth = read_rows(&[fixture::header(), row.clone()]).expect("decodes");
    let Row::Session(start) = &truth.rows[0].row else {
        panic!("a session row");
    };
    assert_eq!(
        (
            start.agent.as_str(),
            start.session.as_str(),
            start.key_group
        ),
        ("a002", "session-a002", 2)
    );
    assert_eq!(start.started_at_unix_ms, fixture::START_MS + 100);

    let mut extra = row.clone();
    extra["reader"] = json!("a002");
    assert!(matches!(
        read_rows(&[fixture::header(), extra]),
        Err(TruthFileError::Decode { line: 2, .. })
    ));
    for key in [
        "world",
        "agent",
        "key_group",
        "session",
        "started_at_unix_ms",
    ] {
        let mut missing = row.clone();
        missing.as_object_mut().unwrap().remove(key).unwrap();
        assert!(
            matches!(
                read_rows(&[fixture::header(), missing]),
                Err(TruthFileError::Decode { line: 2, .. })
            ),
            "a session row without {key} is refused"
        );
    }
    let mut other_world = row;
    other_world["world"] = json!("swarm-other");
    assert!(matches!(
        read_rows(&[fixture::header(), other_world]),
        Err(TruthFileError::OtherWorld { line: 2, .. })
    ));
}
