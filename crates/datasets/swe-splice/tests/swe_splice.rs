//! The splice generator: file writes planted as channel transmissions in
//! SWE trajectories, on the synthetic Open-SWE shards (the open-swe crate's
//! `tests/fixtures/open_swe`). Ported from crosstalk-eval's
//! `tests/swe_splice.rs` at 7f8a2fb.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::helpers::chat::{ChatMessage, bodies};
use a2a_bench_corpus::helpers::parquet_rows::ParquetRows;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_open_swe::{COLUMNS, OpenSweRow, discover};
use a2a_bench_dataset_swe_splice::path::normalize_path;
use a2a_bench_dataset_swe_splice::read::{OBSERVATION, numbered, view_banner};
use a2a_bench_dataset_swe_splice::variant::perturb;
use a2a_bench_dataset_swe_splice::write::{heredocs, resolve, writes};
use a2a_bench_dataset_swe_splice::{
    DATASET, DEFAULT_WORKDIR, Options, Pooled, ReadForm, SPLICES, VERSION, Variant, WriteForm,
    insertion_points, plan, rewrite_workdir, source, workdir, world,
};
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::labels::{
    CarrierKind, Codec, Label, MatchNeed, Route, Tier, TransmissionFields,
};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::{AssistantPart, Body, ToolArguments, ToolPart};
use a2a_bench_format::resource::Resource;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../open-swe/tests/fixtures/open_swe")
}

fn pool() -> Vec<Pooled> {
    let mut out = Vec::new();
    for shard in discover(&root(), None, &[]).unwrap() {
        let rows = ParquetRows::<OpenSweRow>::open(&root().join(&shard.relative), COLUMNS).unwrap();
        for row in rows {
            let (row, record) = row.unwrap();
            out.push(Pooled::new(shard.clone(), row, record));
        }
    }
    out
}

fn by_repo<'a>(pool: &'a [Pooled], repo: &str, harness: &str) -> &'a Pooled {
    pool.iter()
        .find(|p| p.record.repo == repo && p.shard.harness == harness)
        .unwrap_or_else(|| panic!("no {repo} in {harness}"))
}

fn splice_worlds(count: usize, seed: u64) -> Vec<World> {
    let options = Options {
        count,
        seed,
        ..Options::default()
    };
    source(&root(), &options, Pace::DEFAULT)
        .unwrap()
        .worlds()
        .map(|w| w.unwrap())
        .collect()
}

fn planted(world: &World) -> Vec<&TransmissionFields> {
    world
        .labels()
        .iter()
        .filter_map(|e| match e {
            Label::Transmission(t) => Some(t.fields()),
            _ => None,
        })
        .collect()
}

fn agent_of<'a>(world: &'a World, exchange: &Exchange) -> &'a str {
    world.agent_of(exchange.id).unwrap().as_str()
}

/// Whitespace folded: runs of whitespace as one space, trimmed (what a
/// `normalized` match compares).
fn fold(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_dataset_is_ct_evals() {
    assert_eq!(DATASET, "swe_splice");
    assert_eq!(VERSION, 1);
    assert_eq!(SPLICES, 40);
    let defaults = Options::default();
    assert_eq!((defaults.count, defaults.seed), (40, 0));
    assert_eq!(defaults.pool_size(), 120);
    assert_eq!(
        Options {
            count: 2,
            ..Options::default()
        }
        .pool_size(),
        16
    );
    assert_eq!(
        defaults.settings(),
        BTreeMap::from([
            ("corpus_seed".to_owned(), Setting::Int(0)),
            ("count".to_owned(), Setting::Int(40)),
        ])
    );
    let listed = Options {
        include: vec!["b".into(), "a".into()],
        ..Options::default()
    };
    assert_eq!(
        listed.settings().get("include"),
        Some(&Setting::List(vec![
            Setting::Text("b".into()),
            Setting::Text("a".into())
        ]))
    );
}

#[test]
fn heredocs_write_whole_files() {
    let found = heredocs(
        "cat > /tmp/a.py << 'EOF'\nprint(1)\nprint(2)\nEOF\npython /tmp/a.py",
        "/w",
    );
    assert_eq!(
        found,
        vec![("/tmp/a.py".to_owned(), "print(1)\nprint(2)\n".to_owned())]
    );
    let after = heredocs("cat <<EOF > rel/b.py\nx = 1\nEOF", "/w");
    assert_eq!(
        after,
        vec![("/w/rel/b.py".to_owned(), "x = 1\n".to_owned())]
    );
    let cd = heredocs("cd /repo && cat > c.py <<\"END\"\ny = 2\nEND", "/w");
    assert_eq!(cd, vec![("/repo/c.py".to_owned(), "y = 2\n".to_owned())]);
    let tabs = heredocs("cat <<-EOF > d.py\n\tindented\n\tEOF", "/w");
    assert_eq!(tabs, vec![("/w/d.py".to_owned(), "indented\n".to_owned())]);
    let two = heredocs("cat > /a <<A\n1\nA\ncat > /b <<B\n2\nB", "/w");
    assert_eq!(two.len(), 2);
    assert!(heredocs("cat >> /tmp/log <<EOF\nmore\nEOF", "/w").is_empty());
    assert!(heredocs("python3 << EOF\nprint(1)\nEOF", "/w").is_empty());
    assert!(heredocs("cat > /tmp/x <<EOF\nnever closed", "/w").is_empty());
    assert!(heredocs("cat notes.txt && cat > /tmp/y <<EOF\nok\nEOF", "/w").len() == 1);
    assert_eq!(resolve("../x/./y.py", "/a/b"), "/a/x/y.py");
    assert_eq!(normalize_path("/a/./b/../c"), "/a/c");
    assert_eq!(normalize_path("/../x"), "/x");
    assert_eq!(normalize_path("rel//y/"), "/rel/y");
}

#[test]
fn writes_are_found_in_every_harness() {
    let pool = pool();
    let openhands = by_repo(&pool, "acme/widgets", "openhands");
    assert_eq!(openhands.workdir, "/workspace/acme__widgets__1.0");
    let found = openhands.writes();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].form, WriteForm::EditorCreate);
    assert_eq!(
        found[0].path,
        "/workspace/acme__widgets__1.0/reproduce_color.py"
    );
    assert!(found[0].content.starts_with("\"\"\"Check that a widget"));
    assert_eq!(found[0].message, 6);

    let mini = by_repo(&pool, "omega/stream", "minisweagent");
    assert_eq!(mini.workdir, DEFAULT_WORKDIR);
    let heredoc = mini.writes();
    assert_eq!(heredoc.len(), 1);
    assert_eq!(heredoc[0].form, WriteForm::Heredoc);
    assert_eq!(heredoc[0].path, "/tmp/repro_stream.py");
    assert!(heredoc[0].content.ends_with("dropped'\n"));

    let relative = by_repo(&pool, "omega/sink", "minisweagent");
    assert_eq!(relative.writes()[0].path, "/testbed/check_sink.py");
    // Too short, or no write at all.
    assert!(
        by_repo(&pool, "zeta/registry", "sweagent")
            .writes()
            .is_empty()
    );
}

#[test]
fn read_forms_follow_the_harness() {
    let pool = pool();
    assert_eq!(
        ReadForm::of(&by_repo(&pool, "acme/widgets", "openhands").record.messages),
        ReadForm::EditorView { observation: false }
    );
    assert_eq!(
        ReadForm::of(&by_repo(&pool, "acme/gadgets", "sweagent").record.messages),
        ReadForm::EditorView { observation: true }
    );
    assert_eq!(
        ReadForm::of(
            &by_repo(&pool, "omega/stream", "minisweagent")
                .record
                .messages
        ),
        ReadForm::ShellCat
    );
}

#[test]
fn views_number_lines_like_cat_n() {
    assert_eq!(numbered("a\nb\n"), "     1\ta\n     2\tb\n");
    assert_eq!(numbered("only"), "     1\tonly\n");
    let form = ReadForm::EditorView { observation: true };
    let (message, (start, end)) = form.result("/testbed/x.py", "id-1", "a\nb\n");
    let text = message.text();
    assert!(text.starts_with(OBSERVATION));
    assert_eq!(
        &text[OBSERVATION.len()..start],
        view_banner("/testbed/x.py")
    );
    assert_eq!(&text[start..end], "     1\ta\n     2\tb\n");
    assert_eq!(message.tool_call_id.as_deref(), Some("id-1"));

    let (shell, (start, end)) = ReadForm::ShellCat.result("/testbed/x.py", "id-2", "a\"q\"\n");
    let text = shell.text();
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["returncode"], 0);
    assert_eq!(parsed["output"], "     1\ta\"q\"\n");
    assert_eq!(&text[start..end], "     1\\ta\\\"q\\\"\\n");

    let call = ReadForm::ShellCat.call("/testbed/x.py", "id-2");
    assert_eq!(call.calls()[0].function.name, "bash");
    assert!(call.calls()[0].arguments().contains("cat -n /testbed/x.py"));
    let view = form.call("/testbed/x.py", "id-1");
    assert_eq!(view.calls()[0].function.name, "str_replace_editor");
}

#[test]
fn spliced_reads_convert_to_a_call_and_its_result() {
    // ct-eval's extractor tests ran crosstalk-flow over these two messages;
    // the bench pins what they convert to.
    for (form, name, json) in [
        (
            ReadForm::EditorView { observation: false },
            "str_replace_editor",
            r#"{"command":"view","path":"/testbed/src/pkg/module.py"}"#,
        ),
        (
            ReadForm::ShellCat,
            "bash",
            r#"{"command":"cat -n /testbed/src/pkg/module.py"}"#,
        ),
    ] {
        let path = "/testbed/src/pkg/module.py";
        let (result, _) = form.result(path, "splice_read", "def f():\n    return 42\n");
        let converted = bodies(&[form.call(path, "splice_read"), result]).unwrap();
        let Body::Assistant(parts) = &converted[0] else {
            panic!("a call");
        };
        let [AssistantPart::ToolCall(call)] = parts.as_slice() else {
            panic!("one tool call: {parts:?}");
        };
        assert_eq!(call.name, name);
        assert_eq!(call.call_id, "splice_read");
        let ToolArguments::Json(arguments) = &call.arguments else {
            panic!("JSON arguments");
        };
        assert_eq!(arguments.as_str(), json);
        let Body::Tool(results) = &converted[1] else {
            panic!("a result");
        };
        let [ToolPart::ToolResult(result)] = results.as_slice() else {
            panic!("one result");
        };
        assert_eq!(result.call_id, "splice_read");
    }
}

#[test]
fn variants_render_and_need_what_they_should() {
    let content = "def f(a, b):\n    return a + b\n";
    assert_eq!(Variant::Exact.render(content), content);
    let perturbed = Variant::Whitespace.render(content);
    assert_ne!(perturbed, content);
    assert_eq!(perturbed, perturb(content));
    assert_eq!(fold(&perturbed), fold(content));
    assert!(perturbed.contains("\treturn  a  +  b  \n"));
    assert_eq!(
        Variant::JsonString.render(content),
        "\"def f(a, b):\\n    return a + b\\n\""
    );
    assert_eq!(
        Variant::Base64.render("hi\n"),
        "aGkK",
        "standard base64 of the text"
    );
    let editor = ReadForm::EditorView { observation: false };
    let shell = ReadForm::ShellCat;
    let reach = |need: MatchNeed| (need, Tier::Construction);
    let decoded = |codecs: Vec<Codec>| reach(MatchNeed::Decoded { codecs });
    assert_eq!(Variant::Exact.need(editor), reach(MatchNeed::Exact));
    assert_eq!(
        Variant::Whitespace.need(editor),
        reach(MatchNeed::Normalized)
    );
    assert_eq!(
        Variant::JsonString.need(editor),
        decoded(vec![Codec::JsonString])
    );
    assert_eq!(Variant::Base64.need(editor), decoded(vec![Codec::Base64]));
    // A shell read arrives inside the harness's JSON `output` string.
    assert_eq!(Variant::Exact.need(shell), decoded(vec![Codec::JsonString]));
    assert_eq!(
        Variant::Whitespace.need(shell),
        decoded(vec![Codec::JsonString])
    );
    assert_eq!(
        Variant::Base64.need(shell),
        decoded(vec![Codec::JsonString, Codec::Base64])
    );
    // A JSON string inside a JSON string: the writer's level is undone on
    // the writer's argument values (crosstalk's INV-1057), so the reader's
    // side needs one level and the label is in reach.
    assert_eq!(
        Variant::JsonString.need(shell),
        decoded(vec![Codec::JsonString])
    );
}

#[test]
fn working_directories_are_made_one() {
    let pool = pool();
    let openhands = by_repo(&pool, "acme/widgets", "openhands");
    let moved = rewrite_workdir(&openhands.record.messages, &openhands.workdir, "/testbed");
    assert_eq!(workdir(&moved), "/testbed");
    let found = writes(&moved, "/testbed");
    assert_eq!(found[0].path, "/testbed/reproduce_color.py");
    assert!(
        moved
            .iter()
            .all(|m| !m.text().contains("/workspace/acme__widgets__1.0"))
    );
    assert_eq!(
        rewrite_workdir(&openhands.record.messages, "/x", "/x"),
        openhands.record.messages
    );
}

#[test]
fn reads_are_spliced_after_a_completed_step() {
    let assistant = |text: &str| ChatMessage {
        role: "assistant".into(),
        content: Some(text.into()),
        ..ChatMessage::default()
    };
    let other = |role: &str| ChatMessage {
        role: role.into(),
        ..ChatMessage::default()
    };
    let messages = vec![
        other("system"),
        other("user"),
        assistant("first"),
        other("tool"),
        assistant("second"),
        assistant("third"),
        other("tool"),
        assistant("fourth"),
    ];
    assert_eq!(insertion_points(&messages), vec![4, 7]);
}

#[test]
fn splices_plant_one_channel_transmission() {
    let worlds = splice_worlds(8, 7);
    assert_eq!(worlds.len(), 8);
    for (number, world) in worlds.iter().enumerate() {
        let variant = Variant::ALL[number % 4];
        assert!(
            world
                .key()
                .as_str()
                .starts_with(&format!("splice-{number:04}-{variant}-")),
            "{}",
            world.key()
        );
        assert_eq!(world.dataset().as_str(), "swe_splice");
        assert_eq!(world.decl().agents.len(), 2);
        let labels = planted(world);
        assert_eq!(labels.len(), 1);
        let label = labels[0];
        assert_eq!(label.id.as_str(), format!("splice/{number}"));
        // Every variant and read form is in reach (a JSON string read
        // through a shell too, see `Variant::need`).
        assert_eq!(label.tier, Tier::Construction);
        assert_eq!(label.carrier, CarrierKind::ToolResult);
        assert!(label.from.as_str().starts_with("sender/"));
        assert!(label.to.as_str().starts_with("reader/"));
        let Route::Channel {
            resource: Resource::File { host: None, path },
        } = &label.route
        else {
            panic!("a splice is a file channel: {:?}", label.route);
        };
        assert!(path.starts_with('/'));
        assert!(
            label.content.text.contains("     1\\t") || label.content.text.starts_with("     1\t")
        );

        // The sender wrote the file, at the same path, before the reader's
        // exchange that first carries it.
        let reader = world.exchange(label.reader_exchange).unwrap();
        let sender = label
            .sender_exchange
            .and_then(|id| world.exchange(id))
            .unwrap();
        assert!(sender.at_us < reader.at_us);
        let file = path.rsplit('/').next().unwrap_or_default();
        let names_file = sender.response.messages.iter().any(|id| {
            let m = world.message(*id).unwrap();
            (0..m.part_count()).any(|p| {
                u16::try_from(p)
                    .ok()
                    .and_then(|p| m.part_text(p).ok())
                    .is_some_and(|text| text.contains(file))
            })
        });
        assert!(names_file, "the writing call names {file}");
        assert!(reader.request.messages.contains(&label.content.at.message));
        // Only the reader's first exchange after the read carries it as new.
        let earlier: Vec<_> = world
            .exchanges()
            .iter()
            .filter(|e| agent_of(world, e) == label.to.as_str() && e.at_us < reader.at_us)
            .collect();
        assert!(
            earlier
                .iter()
                .all(|e| !e.request.messages.contains(&label.content.at.message))
        );
        // The planted pair has no control at the planted exchange.
        assert!(world.labels().iter().all(|e| match e {
            Label::NegativeControl(c) => {
                let l = c.fields();
                !(l.from == label.from && l.reader_exchange == Some(label.reader_exchange))
            }
            _ => true,
        }));
    }
}

#[test]
fn splices_pair_different_repositories_deterministically() {
    let pool = pool();
    let mut variants = BTreeSet::new();
    for number in 0..12 {
        let first = plan(&pool, number, 3).unwrap();
        let again = plan(&pool, number, 3).unwrap();
        assert_eq!(
            (
                first.sender,
                first.reader,
                first.write,
                first.insert_at,
                &first.call_id
            ),
            (
                again.sender,
                again.reader,
                again.write,
                again.insert_at,
                &again.call_id
            )
        );
        assert_ne!(
            pool[first.sender].record.repo,
            pool[first.reader].record.repo
        );
        variants.insert(first.variant);
    }
    assert_eq!(variants.len(), 4);
    let truth = |seed| -> Vec<Vec<Label>> {
        splice_worlds(6, seed)
            .iter()
            .map(|w| w.labels().to_vec())
            .collect()
    };
    assert_eq!(truth(11), truth(11));
    assert_ne!(truth(11), truth(12));
}

#[test]
fn a_splice_world_builds_from_a_chosen_pair() {
    let pool = pool();
    let mut chosen = plan(&pool, 0, 0).unwrap();
    // Force an OpenHands writer into a SWE-agent reader (which has the
    // editor, so it views the file).
    chosen.sender = pool
        .iter()
        .position(|p| p.record.repo == "acme/widgets")
        .unwrap_or_default();
    chosen.write = 0;
    chosen.reader = pool
        .iter()
        .position(|p| p.record.repo == "acme/gadgets" && p.shard.harness == "sweagent")
        .unwrap_or_default();
    chosen.insert_at = 4;
    let built = world(&pool, &chosen, Pace::DEFAULT).unwrap();
    let label = planted(&built)[0];
    assert_eq!(
        label.route,
        Route::Channel {
            resource: Resource::File {
                host: None,
                path: "/testbed/reproduce_color.py".into()
            }
        }
    );
    assert_eq!(label.needs, MatchNeed::Exact);
    assert!(
        label
            .content
            .text
            .starts_with("     1\t\"\"\"Check that a widget")
    );
}

#[test]
fn the_read_arrives_within_the_live_correlation_window() {
    // The reader's read call is one step after the sender's write and its
    // result one step later still: 2 to 10 s with the default pace, inside
    // crosstalk's live 60 s correlation window.
    for world in splice_worlds(8, 7) {
        let label = planted(&world)[0];
        let sender = label.sender_exchange.unwrap();
        let at = |id| world.exchange(id).unwrap().at_us.as_micros();
        let gap = at(label.reader_exchange) - at(sender);
        assert!(
            (2_000_000..=10_000_000).contains(&gap),
            "{}: the read result arrives {gap} µs after the write",
            world.key()
        );
    }
}

#[test]
fn the_pool_records_the_shards_it_read() {
    let options = Options {
        count: 2,
        ..Options::default()
    };
    let mut splices = source(&root(), &options, Pace::DEFAULT).unwrap();
    assert!(splices.files_read().is_empty());
    let pool = splices.pool().unwrap();
    // 16 wanted, at most 6 per shard: all six rows of the fixture.
    assert_eq!(pool.len(), 6);
    assert_eq!(splices.files_read().len(), 3);
}

#[test]
fn splice_worlds_export_and_reexport_byte_identically() {
    use a2a_bench_corpus::export::{
        EXCHANGES_FILE, LABELS_FILE, MESSAGES_FILE, ManifestInfo, export,
    };
    use a2a_bench_corpus::split::Selection;
    use a2a_bench_format::ids::DatasetId;
    use a2a_bench_format::manifest::{Converter, Source};

    let options = Options {
        count: 6,
        seed: 2,
        ..Options::default()
    };
    let run = |dir: &Path| {
        let mut splices = source(&root(), &options, Pace::DEFAULT).unwrap();
        let info = |digest| ManifestInfo {
            dataset: DatasetId::new(DATASET).unwrap(),
            dataset_version: VERSION,
            source: Source {
                path: "open-swe-traces".into(),
                revision: String::new(),
                digest,
            },
            converter: Converter {
                version: "0.1.0".into(),
                git: "test".into(),
            },
            selection: options.settings(),
            pace: Pace::DEFAULT.settings(),
        };
        // The digest is of the files the pool reads: read it first.
        splices.pool().unwrap();
        let digest = splices.files_read().digest(&root()).unwrap();
        let exported = export(&mut splices, dir, info(digest), &Selection::Unsplit).unwrap();
        assert!(exported.failures.is_empty());
        assert_eq!(exported.manifest.worlds.len(), 6);
    };
    let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    run(first.path());
    run(second.path());
    for file in [MESSAGES_FILE, EXCHANGES_FILE, LABELS_FILE, "manifest.json"] {
        assert_eq!(
            std::fs::read(first.path().join(file)).unwrap(),
            std::fs::read(second.path().join(file)).unwrap(),
            "{file}"
        );
    }
}
