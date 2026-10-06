//! The shell model and the converter's labels: every label's resource is
//! what the shell model records for both of its calls; write outcomes;
//! the JSON string codec.

use std::collections::BTreeMap;

use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_ai_village::shell::outcome::WriteOutcome;
use a2a_bench_dataset_ai_village::shell::{Access, Op, Payload, Shell};
use a2a_bench_dataset_ai_village::text::{json_escape, need};
use a2a_bench_dataset_ai_village::time::{Day, parse_timestamp};
use a2a_bench_dataset_ai_village::window::repo::{AccessLog, Link, TurnRef};
use a2a_bench_dataset_ai_village::{AiVillageSource, Mode, Stats};
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::labels::{Codec, MatchNeed, Route, Tier, TransmissionFields};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ToolArguments, ToolCall, ToolExecution,
};
use serde_json::json;

use super::common::{channel_labels, exchange, key};
use super::fixture::{self, ALICE, BOB, anthropic, bash, responses, table, turn};

const PLAN: &str = "Phase two: migrate the tracker to the shared garden schema by Friday";
const ISSUE_TITLE: &str = "Tracker: the start page loses its filters on reload";
const COMMENT: &str = "The parser now handles nested quotes in every field";
const NOTE: &str = "Signal garden links are fixed on the staging site now";
const HOME: &str = "/home/computeruse";

/// The commands, in order: (turn, agent, time, command, output).
fn script() -> Vec<(&'static str, &'static str, &'static str, String, String)> {
    let at = |minute: u32| format!("2026-07-13 16:{minute:02}:00.0");
    let pushed =
        "To https://github.com/ai-village-agents/tracker.git\n   1a2b3c4..5d6e7f8  main -> main";
    vec![
        (
            "A1",
            ALICE,
            "00",
            format!("cd {HOME} && git clone https://github.com/ai-village-agents/tracker.git"),
            "Cloning into 'tracker'...".to_owned(),
        ),
        (
            "B1",
            BOB,
            "01",
            format!("cd {HOME} && git clone git@github.com:AI-Village-Agents/tracker.git"),
            "Cloning into 'tracker'...".to_owned(),
        ),
        (
            "A2",
            ALICE,
            "05",
            format!(
                "cd {HOME}/tracker && cat > docs/plan.md <<'EOF'\n{PLAN}\nEOF\ngit add docs/plan.md && git commit -m 'Add the plan' && git push origin main"
            ),
            pushed.to_owned(),
        ),
        (
            "B2",
            BOB,
            "10",
            format!("cd {HOME}/tracker && git pull"),
            "From github.com:AI-Village-Agents/tracker\n   1a2b3c4..5d6e7f8  main -> origin/main\nFast-forward\n docs/plan.md | 1 +".to_owned(),
        ),
        (
            "B3",
            BOB,
            "12",
            format!("sed -n '1,40p' {HOME}/tracker/docs/plan.md"),
            format!("{PLAN}\n"),
        ),
        (
            "A3",
            ALICE,
            "15",
            format!(
                "gh issue create -R ai-village-agents/tracker --title '{ISSUE_TITLE}' --body 'Steps to reproduce below'"
            ),
            "https://github.com/ai-village-agents/tracker/issues/7".to_owned(),
        ),
        (
            "B4",
            BOB,
            "20",
            "gh issue list -R ai-village-agents/tracker".to_owned(),
            format!("7\tOPEN\t{ISSUE_TITLE}\tabout 5 minutes ago"),
        ),
        (
            "A4",
            ALICE,
            "25",
            format!("gh issue comment 7 -R ai-village-agents/tracker --body '{COMMENT}'"),
            "https://github.com/ai-village-agents/tracker/issues/7#issuecomment-1".to_owned(),
        ),
        (
            "B5",
            BOB,
            "30",
            "gh issue view 7 -R ai-village-agents/tracker --comments".to_owned(),
            format!("alice (Member) • 5m\n{COMMENT}\n"),
        ),
        (
            "A5",
            ALICE,
            "35",
            format!("glab issue note 3 -R ai-village-agents/village/signal-garden -m '{NOTE}'"),
            "https://gitlab.com/ai-village-agents/village/signal-garden/-/issues/3#note_1"
                .to_owned(),
        ),
        (
            "B6",
            BOB,
            "40",
            "curl -s https://gitlab.com/api/v4/projects/ai-village-agents%2Fvillage%2Fsignal-garden/issues/3/notes".to_owned(),
            format!("[{{\"id\": 1, \"body\": \"{NOTE}\"}}]"),
        ),
        (
            "B7",
            BOB,
            "45",
            "curl -sL https://ai-village-agents.github.io/tracker/".to_owned(),
            "<html><body>Tracker</body></html>".to_owned(),
        ),
        (
            "B8",
            BOB,
            "50",
            "curl -s https://raw.githubusercontent.com/ai-village-agents/tracker/main/docs/plan.md"
                .to_owned(),
            format!("{PLAN}\n"),
        ),
        ("B9", BOB, "55", "ls".to_owned(), "README.md".to_owned()),
    ]
    .into_iter()
    .map(|(id, agent, minute, command, output)| {
        let minute: u32 = minute.parse().unwrap_or_else(|e| panic!("{e}"));
        (id, agent, leak(at(minute)), command, output)
    })
    .collect()
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn dataset() -> tempfile::TempDir {
    let dir = fixture::dir();
    let root = dir.path();
    fixture::base(root);
    table(
        root,
        "computer_use_sessions",
        &[
            json!({"id": "sA", "agent_id": ALICE, "session_goal": "g"}),
            json!({"id": "sB", "agent_id": BOB, "session_goal": "g"}),
        ],
    );
    let turns: Vec<serde_json::Value> = script()
        .into_iter()
        .map(|(id, agent, at, command, output)| {
            let call = format!("call_{id}");
            let (session, messages) = if agent == ALICE {
                (
                    "sA",
                    anthropic("Working", &call, "bash", json!({"command": command})),
                )
            } else {
                (
                    "sB",
                    responses("Working", &call, "bash", json!({"command": command})),
                )
            };
            turn(
                id,
                session,
                at,
                bash(&command),
                messages,
                Some(&output),
                None,
            )
        })
        .collect();
    table(root, "computer_use_turns", &turns);
    table(root, "events", &[]);
    table(root, "chat_messages", &[]);
    table(root, "agent_memories", &[]);
    dir
}

/// What the shell model records for every turn, each agent's commands in
/// order through its own shell.
fn extracted() -> BTreeMap<&'static str, Vec<Access>> {
    let mut shells: BTreeMap<&str, Shell> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (id, agent, _, command, output) in script() {
        let shell = shells.entry(agent).or_default();
        out.insert(id, shell.accesses(&command, &output));
    }
    out
}

/// The read turn and the write turn a label's source names.
fn turns(label: &TransmissionFields) -> (String, String) {
    let path = label.source.path().trim_start_matches('/');
    let (read, rest) = path
        .split_once("#write=")
        .unwrap_or_else(|| panic!("{path}"));
    let write = rest.split('&').next().unwrap_or(rest);
    (read.to_owned(), write.to_owned())
}

#[test]
fn every_label_resource_is_what_the_extractor_records_for_both_calls() {
    let dir = dataset();
    let day = Day::parse("2026-07-13").unwrap_or_else(|e| panic!("{e}"));
    let mut source = AiVillageSource::open(dir.path(), Mode::Window { from: day, to: day })
        .unwrap_or_else(|e| panic!("{e}"));
    let worlds: Vec<World> = source
        .worlds()
        .map(|w| w.unwrap_or_else(|e| panic!("{e}")))
        .collect();
    assert_eq!(worlds.len(), 1);
    let extracted = extracted();
    let mut seen: Vec<(String, String, bool)> = Vec::new();
    for (label, access_only) in channel_labels(&worlds[0]) {
        let Route::Channel { resource } = &label.route else {
            panic!("{:?}", label.route);
        };
        let (read, write) = turns(label);
        let wrote = extracted[write.as_str()]
            .iter()
            .any(|c| c.resource().as_ref() == Some(resource) && c.op.is_write());
        let reads = extracted[read.as_str()]
            .iter()
            .any(|c| c.resource().as_ref() == Some(resource) && c.op == Op::Read);
        assert!(
            wrote,
            "{write} does not write {resource:?}: {:?}",
            extracted[write.as_str()]
        );
        assert!(
            reads,
            "{read} does not read {resource:?}: {:?}",
            extracted[read.as_str()]
        );
        // An access-only label is a push the shell records without
        // content; a content label's write carries what its author typed.
        let unseen = extracted[write.as_str()].iter().any(|c| {
            c.resource().as_ref() == Some(resource) && c.payload() == Some(&Payload::Unseen)
        });
        assert_eq!(unseen, access_only, "{write} -> {read}");
        seen.push((read, key(resource), access_only));
    }
    seen.sort();
    let expected = vec![
        ("B2", "repo://github.com/ai-village-agents/tracker", true),
        (
            "B3",
            "file://github.com/ai-village-agents/tracker/docs/plan.md",
            false,
        ),
        (
            "B4",
            "https://github.com/ai-village-agents/tracker/issues",
            false,
        ),
        (
            "B5",
            "https://github.com/ai-village-agents/tracker/issues/7",
            false,
        ),
        (
            "B6",
            "https://gitlab.com/ai-village-agents/village/signal-garden/-/issues/3",
            false,
        ),
        ("B7", "repo://github.com/ai-village-agents/tracker", true),
        (
            "B8",
            "file://github.com/ai-village-agents/tracker/docs/plan.md",
            false,
        ),
    ];
    let expected: Vec<(String, String, bool)> = expected
        .into_iter()
        .map(|(read, key, access)| (read.to_owned(), key.to_owned(), access))
        .collect();
    assert_eq!(seen, expected);
    let Stats::Window(stats) = source.stats() else {
        panic!("not a window");
    };
    assert_eq!(stats.repo_labels, 5);
    assert_eq!(stats.repo_access_only_labels, 2);
    assert_eq!(stats.access.unextracted_commands, 0);
}

#[test]
fn an_access_only_label_covers_the_whole_read_output() {
    let dir = dataset();
    let day = Day::parse("2026-07-13").unwrap_or_else(|e| panic!("{e}"));
    let mut source = AiVillageSource::open(dir.path(), Mode::Window { from: day, to: day })
        .unwrap_or_else(|e| panic!("{e}"));
    let world = source
        .worlds()
        .next()
        .unwrap_or_else(|| panic!("no world"))
        .unwrap_or_else(|e| panic!("{e}"));
    let pages = channel_labels(&world)
        .into_iter()
        .find(|(label, access)| *access && turns(label).0 == "B7")
        .map(|(label, _)| label)
        .unwrap_or_else(|| panic!("no Pages label"));
    assert_eq!(pages.content.text, "<html><body>Tracker</body></html>");
    assert_eq!(pages.needs, MatchNeed::Exact);
    // At Bob's next call, the one whose request carries the output.
    let reader = exchange(&world, pages.reader_exchange);
    assert_eq!(reader.source.path(), "/B8");
}

fn writes_of(shell: &mut Shell, command: &str, output: &str) -> Op {
    let mut accesses = shell.accesses(command, output);
    assert_eq!(accesses.len(), 1, "{command}: {accesses:?}");
    accesses.remove(0).op
}

fn outcome(op: Op) -> WriteOutcome {
    match op {
        Op::Write { outcome, .. } => outcome,
        Op::Read => panic!("a read"),
    }
}

#[test]
fn write_outcomes_are_the_extractors_judgement_of_the_output() {
    let mut shell = Shell::default();
    let rejected = writes_of(
        &mut shell,
        "git push https://github.com/o/r.git main",
        "To https://github.com/o/r.git\n ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs",
    );
    assert_eq!(outcome(rejected), WriteOutcome::Rejected);
    let delivered = writes_of(
        &mut shell,
        "git push https://github.com/o/r.git main",
        "To https://github.com/o/r.git\n   1a2b3c4..5d6e7f8  main -> main",
    );
    assert_eq!(
        delivered,
        Op::Write {
            outcome: WriteOutcome::Delivered,
            payload: Payload::Unseen
        }
    );
    let denied = writes_of(
        &mut shell,
        "gh issue comment 5 -R o/r --body 'This one did not go through at all'",
        "GraphQL: Could not resolve to an issue or pull request with the number of 5.",
    );
    assert_eq!(outcome(denied), WriteOutcome::Rejected);
    let commented = writes_of(
        &mut shell,
        "gh issue comment 5 -R o/r --body 'This one went through just fine'",
        "https://github.com/o/r/issues/5#issuecomment-3",
    );
    assert_eq!(outcome(commented), WriteOutcome::Delivered);
    let curl_error = writes_of(
        &mut shell,
        "curl -X POST https://down.example/notes -d x=1",
        "curl: (6) Could not resolve host: down.example",
    );
    assert_eq!(outcome(curl_error), WriteOutcome::Rejected);
    // A read needs a delivered result.
    assert!(
        shell
            .accesses(
                "curl https://down.example/x",
                "curl: (7) Failed to connect to down.example port 443"
            )
            .is_empty()
    );
}

#[test]
fn rejected_writes_never_pair_and_pushes_pair_access_only() {
    let day = Day::parse("2026-07-13").unwrap_or_else(|e| panic!("{e}"));
    let at = |text: &str| parse_timestamp(text).unwrap_or_else(|e| panic!("{e}"));
    let mut log = AccessLog::default();
    let turn = |agent: &'static str, id: &'static str, when: &str| TurnRef {
        agent,
        turn: id,
        session: agent,
        at: at(when),
        day,
    };
    log.command(
        turn("alice", "t1", "2026-07-13 16:00:00"),
        "gh issue comment 5 -R o/r --body 'An update that was refused by the forge'",
        "HTTP 403: Resource not accessible by integration",
    );
    log.command(
        turn("bob", "t2", "2026-07-13 16:05:00"),
        "gh issue view 5 -R o/r",
        "title: Parser\nAn update that was refused by the forge",
    );
    log.command(
        turn("alice", "t3", "2026-07-13 16:10:00"),
        "gh issue comment 6 -R o/r --body 'A second update that the forge accepted'",
        "https://github.com/o/r/issues/6#issuecomment-1",
    );
    log.command(
        turn("bob", "t4", "2026-07-13 16:15:00"),
        "gh issue view 6 -R o/r",
        "A second update that the forge accepted",
    );
    log.command(
        turn("alice", "t5", "2026-07-13 16:20:00"),
        "git push https://github.com/o/r.git main",
        "To https://github.com/o/r.git\n   1a2b3c4..5d6e7f8  main -> main",
    );
    log.command(
        turn("bob", "t6", "2026-07-13 16:25:00"),
        "git fetch https://github.com/O/R",
        "From https://github.com/O/R\n * branch main -> FETCH_HEAD",
    );
    log.finish();
    assert_eq!(log.stats.writes, 3);
    assert_eq!(log.stats.rejected_writes, 1);
    assert_eq!(log.stats.unseen_writes, 1);
    let pairs: Vec<(&str, &str, Link)> = log
        .pairs
        .iter()
        .map(|pair| {
            (
                log.records[pair.write].turn.as_str(),
                log.records[pair.read].turn.as_str(),
                pair.link,
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![("t3", "t4", Link::Content), ("t5", "t6", Link::AccessOnly)]
    );
    assert_eq!(log.stats.pairs_access_only, 1);
}

fn assistant(parts: Vec<AssistantPart>) -> Message {
    Message::new(Body::Assistant(parts)).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn escaped_reads_need_the_json_string_codec() {
    let content = "He said \"meet at nine\"\nby the mill";
    let escaped = "He said \\\"meet at nine\\\"\\nby the mill";
    // The sender wrote the text itself: the reader's escaped bytes decode
    // to it.
    let text = assistant(vec![AssistantPart::Text {
        text: content.to_owned(),
    }]);
    assert_eq!(
        need(&text, escaped),
        MatchNeed::Decoded {
            codecs: vec![Codec::JsonString]
        }
    );
    // The sender's tool call carries the same escaped bytes: exact.
    let call = assistant(vec![AssistantPart::ToolCall(ToolCall {
        call_id: "c".to_owned(),
        name: "send_message_back_to_chat".to_owned(),
        arguments: ToolArguments::Json(
            CanonicalJson::canonicalize(&format!("{{\"message\":\"{escaped}\"}}"))
                .unwrap_or_else(|e| panic!("{e}")),
        ),
        execution: ToolExecution::Client,
    })]);
    assert_eq!(need(&call, escaped), MatchNeed::Exact);
    // Nothing to unescape: exact or not, never the codec.
    let plain = assistant(vec![AssistantPart::Text {
        text: "meet at nine".to_owned(),
    }]);
    assert_eq!(need(&plain, "meet at nine"), MatchNeed::Exact);
    assert_eq!(need(&plain, "MEET  at nine"), MatchNeed::Normalized);
    assert_eq!(need(&plain, "an unrelated sentence"), MatchNeed::Semantic);
}

#[test]
fn escapes_need_one_string_level_and_two_are_out_of_reach() {
    let content = "She wrote \"bring the ledger\" and left early";
    let once = json_escape(content);
    let twice = json_escape(&once);
    let raw = assistant(vec![AssistantPart::Text {
        text: content.to_owned(),
    }]);
    // Delivered escaped once, with its whitespace re-wrapped: one string
    // level undone and folded.
    let rewrapped = once.replace(' ', "  ");
    assert_eq!(need(&raw, &rewrapped), MatchNeed::json_string());
    // Escaped twice: no spec decoder undoes two levels.
    let deep = need(&raw, &twice);
    assert_eq!(deep, MatchNeed::two_string_levels());
    assert_eq!(deep.tier(Tier::Structural), Tier::OutOfReach);
    assert_eq!(
        MatchNeed::json_string().tier(Tier::Structural),
        Tier::Structural
    );
}
