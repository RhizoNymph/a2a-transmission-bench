//! A small synthetic swarm run, written as the files the bench reads: the
//! truth file and the capture (bench `messages.jsonl` and
//! `exchanges.jsonl`, as crosstalk's adapter writes them from the
//! gateway's records). Built here, never copied. ct-eval's fixture wrote
//! the same run as the gateway's exchange log and blobs.
//!
//! Three agents, one session each, on the wiki at `http://wiki:8090`:
//!
//! | session | turn | request ends with | response |
//! | --- | --- | --- | --- |
//! | a001 | 0 | the task | PUT p1 (`P1`) |
//! | a001 | 1 | PUT p1's result | PUT p2 (`P2`) |
//! | a001 | 2 | PUT p2's result | GET p1 |
//! | a001 | 3 | GET p1 → `P1` (a self-read) | text |
//! | a002 | 0 | the task | GET p1 |
//! | a002 | 1 | GET p1 → `P1` (a transmission from a001) | GET p1 |
//! | a002 | 2 | GET p1 → `P1` (a reread) | GET p9 |
//! | a002 | 3 | GET p9 → not found (a miss) | text |
//! | a003 | 0 | the task | GET p2 |
//! | a003 | 1 | GET p2 → `P2` (a transmission from a001) | GET p1 |
//! | a003 | 2 | GET p1 → `P1` (a transmission from a001) | text |
//!
//! The run's exchanges start ten seconds apart from `T0` + 10 s (the
//! header's start is `T0`), inside the run window its rows imply: the rows
//! are timed near the run's end (`T0` + 100 s, the miss at `T0` + 101 s),
//! plus the default minute of slack. [`Shape::prior`] also captures, an
//! hour before, an earlier run that reused a002's session id.

#![allow(dead_code)]

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use a2a_bench_dataset_demo_swarm::Inputs;
use a2a_bench_dataset_demo_swarm::schema::HexDigest;
use a2a_bench_format::exchange::{Client, Exchange, Fidelity, Request, Response, WorldDecl};
use a2a_bench_format::files::{ExchangeRow, Exchanges, MessageRow, Messages, WorldOnly};
use a2a_bench_format::ids::{DatasetId, ExchangeId, MessageId, SourceRef, WorldKey};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::jsonl::{BasicHeader, FileWriter};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use a2a_bench_format::time::Timestamp;
use serde_json::json;

pub const WORLD: &str = "swarm-fixture";
pub const HEADLINE: &str = "demo-swarm/headline";
pub const P1: &str = "# Plan\nMeet at the \"north\" gate at noon; bring the ledger.";
pub const P2: &str = "page two holds a plain sentence with no escapes at all";
pub const NOT_FOUND: &str = "404 page not found";
/// The header's start, in Unix milliseconds.
pub const START_MS: u64 = 1_790_812_800_000;

pub fn url(page: &str) -> String {
    format!("http://wiki:8090/pages/{page}")
}

pub fn blake3_hex(text: &str) -> String {
    HexDigest::blake3_of(text.as_bytes()).to_string()
}

/// One captured exchange: its id, its response message and the tool
/// message it ended its request with (if any).
#[derive(Debug, Clone, Copy)]
pub struct Turn {
    pub id: ExchangeId,
    pub response: MessageId,
    pub last_tool: Option<MessageId>,
}

/// An agent's session: its exchanges in order, each request the whole
/// history so far.
struct Agent {
    session: String,
    credential: String,
    turns: Vec<Turn>,
    history: Vec<Message>,
}

impl Agent {
    fn new(name: &str) -> Self {
        Self {
            session: format!("session-{name}"),
            credential: format!("k:{}", blake3_hex(name)),
            turns: Vec::new(),
            history: vec![
                message(Body::System(vec![SystemPart::Text {
                    text: "You are a swarm agent.".to_owned(),
                }])),
                message(Body::User(vec![UserPart::Text {
                    text: "Keep the wiki.".to_owned(),
                }])),
            ],
        }
    }
}

pub fn message(body: Body) -> Message {
    Message::new(body).unwrap_or_else(|error| panic!("a message: {error}"))
}

fn call(id: &str, arguments: &serde_json::Value) -> Message {
    let json = CanonicalJson::canonicalize(&arguments.to_string())
        .unwrap_or_else(|error| panic!("arguments: {error}"));
    message(Body::Assistant(vec![AssistantPart::ToolCall(ToolCall {
        call_id: id.to_owned(),
        name: "http_request".to_owned(),
        arguments: ToolArguments::Json(json),
        execution: ToolExecution::Client,
    })]))
}

fn put(id: &str, page: &str, body: &str) -> Message {
    call(
        id,
        &json!({"method": "PUT", "url": url(page), "body": body}),
    )
}

fn get(id: &str, page: &str) -> Message {
    call(id, &json!({"method": "GET", "url": url(page)}))
}

fn text(text: &str) -> Message {
    message(Body::Assistant(vec![AssistantPart::Text {
        text: text.to_owned(),
    }]))
}

fn result(id: &str, text: &str) -> Message {
    message(Body::Tool(vec![ToolPart::ToolResult(ToolResult {
        call_id: id.to_owned(),
        content: vec![ResultContent::Text {
            text: text.to_owned(),
        }],
        outcome: ToolOutcome::Success,
    })]))
}

/// How the fixture's capture is shaped.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shape {
    /// An earlier run, an hour before, reusing a002's session id.
    pub prior: bool,
    /// The run's clock is this many seconds behind the header's.
    pub behind_secs: u64,
    /// Each exchange's `client.turn` is this much off its ordinal.
    pub turn_offset: u32,
    /// An exchange of a session no truth row names (an unrelated client).
    pub stranger: bool,
}

struct Log {
    exchanges: Vec<Exchange>,
    messages: Vec<Message>,
    /// When the exchanges' run started, in microseconds.
    base: u64,
    /// Ten-second steps since `base` of the last exchange.
    clock: u64,
    counter: u128,
    turn_offset: u32,
}

impl Log {
    /// One exchange of `agent`: the history as request, then `response`;
    /// afterwards the history holds the response and `result`, if any.
    fn turn(&mut self, agent: &mut Agent, response: Message, result: Option<Message>) {
        self.clock += 1;
        let at = self.base + self.clock * 10_000_000;
        self.counter += 1;
        let id = ExchangeId::from_raw((u128::from(at / 1000) << 80) | self.counter);
        let last_tool = agent
            .history
            .last()
            .filter(|held| matches!(held.body(), Body::Tool(_)))
            .map(Message::id);
        let ordinal = u32::try_from(agent.turns.len()).unwrap_or(u32::MAX);
        agent.turns.push(Turn {
            id,
            response: response.id(),
            last_tool,
        });
        for held in agent.history.iter().chain([&response]) {
            if !self.messages.iter().any(|known| known.id() == held.id()) {
                self.messages.push(held.clone());
            }
        }
        self.exchanges.push(Exchange {
            id,
            at_us: Timestamp::from_micros(at),
            client: Client {
                credential: agent.credential.clone(),
                session: Some(agent.session.clone()),
                turn: Some(ordinal + self.turn_offset),
                vendor: Some("anthropic".to_owned()),
                model: Some("claude-sonnet".to_owned()),
            },
            request: Request {
                messages: agent.history.iter().map(Message::id).collect(),
                tools: None,
            },
            response: Response {
                messages: vec![response.id()],
                stop: Some("tool_use".to_owned()),
            },
            fidelity: Fidelity::Exact,
            source: SourceRef::new("exchange-log.jsonl", format!("exchange/{id}")),
        });
        agent.history.push(response);
        if let Some(result) = result {
            agent.history.push(result);
        }
    }
}

/// The truth file's header line.
pub fn header() -> serde_json::Value {
    json!({"kind": "header", "version": 2, "world": WORLD, "run": "01J00000000000000000000000",
        "seed": 42, "agents": 3, "keys": 3, "agents_per_key": 1, "claude_code_shape": true,
        "started_at_unix_ms": START_MS, "gateway_url": "http://crosstalk:8080/anthropic",
        "wiki_url": "http://wiki:8090"})
}

/// A delivery row of `kind`.
pub fn delivery(
    kind: &str,
    writer: (&str, &str, u32, &str),
    reader: (&str, &str, u32, &str),
    page: &str,
    text: &str,
) -> serde_json::Value {
    let (writer, writer_session, writer_turn, writer_call) = writer;
    let (reader, reader_session, reader_turn, reader_call) = reader;
    json!({"kind": kind, "world": WORLD, "writer": writer, "reader": reader, "page": page,
        "version": 1, "writer_key_group": 0, "reader_key_group": 1,
        "writer_session": writer_session, "writer_turn": writer_turn, "writer_tool_use_id": writer_call,
        "reader_session": reader_session, "reader_turn": reader_turn, "reader_tool_use_id": reader_call,
        "route": {"kind": "channel", "url": url(page)}, "carrier": "tool_result",
        "read_tool": {"name": "http_request", "input": {"method": "GET", "url": url(page)}},
        "content": {"blake3": blake3_hex(text), "sha256": "00".repeat(32),
            "excerpt": text.chars().take(80).collect::<String>(),
            "at": {"message": 3, "block": 0, "tool_use_id": reader_call}},
        "at_ms": 100_000, "at_unix_ms": START_MS + 100_000,
        "written_at_unix_ms": START_MS + 99_500, "read_at_unix_ms": START_MS + 100_000})
}

/// The truth rows the fixture's run implies, plus two the capture
/// contradicts: a003's read of p1 named at turn 1 (it is at turn 2), and
/// a003's read of p2 claimed with another body's hash.
pub fn truth_rows() -> Vec<serde_json::Value> {
    let a001 = |turn, call| ("a001", "session-a001", turn, call);
    let a002 = |turn, call| ("a002", "session-a002", turn, call);
    let a003 = |turn, call| ("a003", "session-a003", turn, call);
    vec![
        header(),
        // Line 2: a001's p1 reaches a002.
        delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a002(1, "toolu_r1"),
            "p1",
            P1,
        ),
        // Line 3: a001's p2 reaches a003.
        delivery(
            "transmission",
            a001(1, "toolu_w2"),
            a003(1, "toolu_r3"),
            "p2",
            P2,
        ),
        // Line 4: a001 reads its own page.
        delivery(
            "self_read",
            a001(0, "toolu_w1"),
            a001(3, "toolu_self"),
            "p1",
            P1,
        ),
        // Line 5: a002 reads p1 again.
        delivery("reread", a001(0, "toolu_w1"), a002(2, "toolu_r2"), "p1", P1),
        // Line 6: a002 reads a page nobody wrote.
        json!({"kind": "miss", "world": WORLD, "reader": "a002", "reader_key_group": 1, "page": "p9",
            "reader_session": "session-a002", "reader_turn": 3, "reader_tool_use_id": "toolu_m",
            "read_tool": {"name": "http_request", "input": {"method": "GET", "url": url("p9")}},
            "at_ms": 101_000, "at_unix_ms": START_MS + 101_000}),
        // Line 7: the turn is off by one.
        delivery(
            "transmission",
            a001(0, "toolu_w1"),
            a003(1, "toolu_r4"),
            "p1",
            P1,
        ),
        // Line 8: the hash is another body's.
        delivery(
            "transmission",
            a001(1, "toolu_w2"),
            a003(1, "toolu_r3"),
            "p2",
            "a different body",
        ),
        json!({"kind": "agent_cluster", "world": WORLD, "key_group": 0, "agents": ["a001"]}),
    ]
}

/// An `unattributed_read` row, in the demo's pinned key order: a read of
/// `page` whose write was never logged.
pub fn unattributed(reader: (&str, &str, u32, &str), page: &str, text: &str) -> serde_json::Value {
    let (reader, reader_session, reader_turn, reader_call) = reader;
    json!({"kind": "unattributed_read", "world": WORLD, "reader": reader, "reader_key_group": 1,
        "page": page, "version": 5, "reader_session": reader_session, "reader_turn": reader_turn,
        "reader_tool_use_id": reader_call,
        "read_tool": {"name": "http_request", "input": {"method": "GET", "url": url(page)}},
        "content": {"blake3": blake3_hex(text), "sha256": "00".repeat(32),
            "excerpt": text.chars().take(80).collect::<String>(),
            "at": {"message": 3, "block": 0, "tool_use_id": reader_call}},
        "at_ms": 100_000, "at_unix_ms": START_MS + 100_000})
}

/// A `session` row in the swarm's pinned key order.
pub fn session(agent: &str, session: &str) -> serde_json::Value {
    json!({"kind": "session", "world": WORLD, "agent": agent, "key_group": 2,
        "session": session, "started_at_unix_ms": START_MS + 100})
}

/// Writes `rows` as JSONL to `path`.
pub fn write_jsonl(path: &Path, rows: &[serde_json::Value]) {
    let mut text = String::new();
    for row in rows {
        text.push_str(&row.to_string());
        text.push('\n');
    }
    std::fs::write(path, text).unwrap_or_else(|error| panic!("write a fixture file: {error}"));
}

/// A fresh, empty directory for one test.
pub fn dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("swarm_truth")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap_or_else(|error| panic!("clear {name}: {error}"));
    }
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("create {name}: {error}"));
    dir
}

/// Everything the fixture wrote, and the ids tests check against.
pub struct Written {
    pub dir: PathBuf,
    pub inputs: Inputs,
    pub a001: Vec<Turn>,
    pub a002: Vec<Turn>,
    pub a003: Vec<Turn>,
    /// The earlier run's exchanges in a002's session, an hour before the
    /// run (empty unless [`Shape::prior`]).
    pub prior_a002: Vec<Turn>,
    /// The exchange of a session no row names ([`Shape::stranger`]).
    pub stranger: Vec<Turn>,
    /// Every captured exchange, in capture order.
    pub exchanges: Vec<Exchange>,
    pub messages: Vec<Message>,
}

/// Writes the whole fixture under `dir`: the truth file holds `truth`.
pub fn write(dir: &Path, truth: &[serde_json::Value]) -> Written {
    write_shaped(dir, truth, Shape::default())
}

/// [`write`], with an earlier run an hour before in the same capture:
/// a002's session id reused, its first two turns (`GET p1`, then the read
/// of `P1` with the same tool use id).
pub fn write_with_prior_run(dir: &Path, truth: &[serde_json::Value]) -> Written {
    write_shaped(
        dir,
        truth,
        Shape {
            prior: true,
            ..Shape::default()
        },
    )
}

/// [`write`], with the run's clock `behind_secs` seconds behind the
/// header's: its exchanges start at `T0` + 10 s − `behind_secs`, ten
/// seconds apart, as when the swarm's host and the gateway's disagree.
pub fn write_skewed(dir: &Path, truth: &[serde_json::Value], behind_secs: u64) -> Written {
    write_shaped(
        dir,
        truth,
        Shape {
            behind_secs,
            ..Shape::default()
        },
    )
}

pub fn write_shaped(dir: &Path, truth: &[serde_json::Value], shape: Shape) -> Written {
    write_as(dir, truth, shape, HEADLINE, WORLD)
}

/// [`write_shaped`], with the capture's files naming `dataset` and `world`.
pub fn write_as(
    dir: &Path,
    truth: &[serde_json::Value],
    shape: Shape,
    dataset: &str,
    world: &str,
) -> Written {
    let start = START_MS * 1000;
    let mut log = Log {
        exchanges: Vec::new(),
        messages: Vec::new(),
        base: start - 3_600_000_000,
        clock: 0,
        counter: 0,
        turn_offset: shape.turn_offset,
    };
    let mut prior_a002 = shape.prior.then(|| Agent::new("a002"));
    if let Some(agent) = &mut prior_a002 {
        log.turn(agent, get("toolu_r1", "p1"), Some(result("toolu_r1", P1)));
        log.turn(agent, get("toolu_r2", "p1"), Some(result("toolu_r2", P1)));
    }
    let prior_a002 = prior_a002.map_or_else(Vec::new, |agent| agent.turns);
    log.base = start - shape.behind_secs * 1_000_000;
    log.clock = 0;
    let mut a001 = Agent::new("a001");
    let mut a002 = Agent::new("a002");
    let mut a003 = Agent::new("a003");
    log.turn(
        &mut a001,
        put("toolu_w1", "p1", P1),
        Some(result("toolu_w1", "ok")),
    );
    log.turn(
        &mut a001,
        put("toolu_w2", "p2", P2),
        Some(result("toolu_w2", "ok")),
    );
    log.turn(
        &mut a001,
        get("toolu_self", "p1"),
        Some(result("toolu_self", P1)),
    );
    log.turn(&mut a001, text("done"), None);
    log.turn(
        &mut a002,
        get("toolu_r1", "p1"),
        Some(result("toolu_r1", P1)),
    );
    log.turn(
        &mut a002,
        get("toolu_r2", "p1"),
        Some(result("toolu_r2", P1)),
    );
    log.turn(
        &mut a002,
        get("toolu_m", "p9"),
        Some(result("toolu_m", NOT_FOUND)),
    );
    log.turn(&mut a002, text("done"), None);
    log.turn(
        &mut a003,
        get("toolu_r3", "p2"),
        Some(result("toolu_r3", P2)),
    );
    log.turn(
        &mut a003,
        get("toolu_r4", "p1"),
        Some(result("toolu_r4", P1)),
    );
    log.turn(&mut a003, text("done"), None);
    let mut stranger = Vec::new();
    if shape.stranger {
        let mut agent = Agent::new("z999");
        log.turn(&mut agent, text("hello"), None);
        stranger = agent.turns;
    }

    let dataset = DatasetId::new(dataset).unwrap_or_else(|error| panic!("{error}"));
    let world = WorldKey::new(world).unwrap_or_else(|error| panic!("{error}"));
    write_capture(dir, &dataset, &world, &log.messages, &log.exchanges);
    write_jsonl(&dir.join("truth.jsonl"), truth);
    Written {
        dir: dir.to_path_buf(),
        inputs: Inputs::in_dir(dir),
        a001: a001.turns,
        a002: a002.turns,
        a003: a003.turns,
        prior_a002,
        stranger,
        exchanges: log.exchanges,
        messages: log.messages,
    }
}

/// Writes `messages.jsonl` and `exchanges.jsonl` of one world into `dir`,
/// declaring no agents (the adapter does not know them).
pub fn write_capture(
    dir: &Path,
    dataset: &DatasetId,
    world: &WorldKey,
    messages: &[Message],
    exchanges: &[Exchange],
) {
    let create = |name: &str| {
        BufWriter::new(
            File::create(dir.join(name)).unwrap_or_else(|error| panic!("create {name}: {error}")),
        )
    };
    let mut writer = FileWriter::<Messages, _>::new(
        create("messages.jsonl"),
        &BasicHeader::new::<Messages>(dataset.clone()),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    writer
        .world(&WorldOnly { key: world.clone() })
        .unwrap_or_else(|error| panic!("{error}"));
    for held in messages {
        writer
            .row(&MessageRow::Message(held.clone()))
            .unwrap_or_else(|error| panic!("{error}"));
    }
    writer.finish().unwrap_or_else(|error| panic!("{error}"));
    let mut writer = FileWriter::<Exchanges, _>::new(
        create("exchanges.jsonl"),
        &BasicHeader::new::<Exchanges>(dataset.clone()),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    writer
        .world(&WorldDecl {
            key: world.clone(),
            agents: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    for exchange in exchanges {
        writer
            .row(&ExchangeRow::Exchange(exchange.clone()))
            .unwrap_or_else(|error| panic!("{error}"));
    }
    writer.finish().unwrap_or_else(|error| panic!("{error}"));
}
