# demo-swarm (`a2a-bench-dataset-demo-swarm`)

Labels for crosstalk's demo swarm: a traffic generator whose agents read
and write wiki pages through crosstalk's gateway, and which writes its own
ground truth (`truth.jsonl`, version 2) as it runs. A run is exported as
`demo-swarm/headline` or `demo-swarm/boilerplate`, as the truth header's
`scenario` says (missing means `headline`). Version `@1` reproduces
crosstalk-eval's `swarm` labels at 7f8a2fb (`src/datasets/swarm_truth/`).

The truth is written by the traffic generator, not by any detector, so it
is a legitimate label source (design §5.3).

## Scope

- The truth file, version 2, decoded strictly: `header`, `session`,
  `transmission`, `self_read`, `reread`, `miss`, `unattributed_read`,
  `agent_cluster`; unknown kinds and fields, missing fields, another
  version, a second header, rows before the header and rows of another
  world are refused.
- The run window `[header start − lead, latest row time + slack]`
  (defaults 5 s and 60 s), both ends inclusive.
- The join of truth rows to the capture's exchanges by session and turn,
  cross-checked by tool use id and the BLAKE3 of the page body.
- Labels for every row kind, each checked against the world as a reader
  checks it, and the typed join-diagnostics table, written as
  `diagnostics.json` beside the labels.
- A `TraceSource` of the one labelled world, and `write_export`, which
  completes the export (the corpus export writer, the capture's manifest
  kept as the export's input view, then `diagnostics.json` with the
  bench's labelling provenance).
- `write_holdout_export`: the same export marked as one release's
  holdout (a whole run is demo-swarm's holdout unit, design §9.2), and the
  capture digest a holdout export's predictions may name
  (`holdout::capture_digest`).

## Non-scope

- Reading crosstalk's gateway formats (exchange log, blob store, exports,
  evidence) and fetching or replaying runs: crosstalk's adapter
  (`ct-bench-detect from-export`) turns a saved run into the capture this
  crate reads, and writes the run's predictions.
- Scoring, gates, predictions and the detection-side diagnostics ct-eval
  kept in the same table (`missing_evidence`, `unknown_detected_agent`,
  `detected_agent_conflict`, `unpredictable`, `outside_run_window`): the
  adapter and the scorer.
- The CLI wiring (`docs/features/cli.md`), including whether a run is a
  holdout run (its seed) and the commitment list.
- Dataset bytes: tests build a synthetic run.

## Inputs

```text
<run>/truth.jsonl       the swarm's ground truth (v2)
<run>/messages.jsonl    the capture: bench messages, one world
<run>/exchanges.jsonl   the capture: bench exchanges, one world
<run>/manifest.json     the capture: its manifest, an input view (no labels digest, counts or notes)
```

What the capture must be (the contract with crosstalk's adapter):

- framed `a2a-bench/1` files with trailers, header `dataset` equal to
  the truth scenario's dataset id (`demo-swarm/<scenario>`);
- exactly one world, keyed by the truth header's `world`; its declared
  agents are ignored (the adapter cannot know them; the truth names them);
- the format's input checks hold over it (every message once and used,
  exchanges once each and in time order);
- each exchange carries the gateway's minted id, `at_us` = the exchange's
  start, `client.session` = the harness session id
  (`x-claude-code-session-id`), and `client.turn` = its 0-based ordinal in
  the session. `client.turn` is only cross-checked (below); the join uses
  the ordinal computed as ct-eval computed it;
- tool results as `tool` messages in request order, the `PUT`'s arguments
  as canonical JSON (the format's normalisation rules), so a result's part
  text is exactly the page body the truth hashes.

- `manifest.json` is the input view the adapter's predictions name
  (`Manifest::digest`): dataset the truth's, version `@1`, exactly the
  truth's world, `selection` equal to the margins used here
  (`run_lead_ms`, `run_slack_ms`), and file digests and exchange count
  that are the capture's own.

The capture may hold more than the run: exchanges outside the run window
(an earlier run reusing the seed's session ids), exchanges of sessions no
truth row names, and exchanges without a session. None of them is
exported; they are counted (`capture` in `diagnostics.json`), and the
outside ones of truth sessions reported. Such a capture labels, but
`write_export` refuses it (`NotTheCapture`): its export's input view would
not be the capture's manifest, so the capture's predictions could not be
scored against it. The adapter's captures hold exactly the run.

## Data and control flow

```text
DemoSwarmSource::open(Inputs, Options)
  read_truth ── truth_file::read (strict schema, header first, one world) ──▶ TruthFile
  scenario.dataset(), header.world
  capture::read(messages, exchanges, dataset, world)
      FileReader per file ── one world ── dataset and key checked ── WorldInputs::new ──▶ Capture
  label::label(truth, file name, capture, options)
      RunWindow::of(truth, margins)
      window::split(capture.exchanges, window, truth_sessions)
          inside ──▶ Sessions::index (by client.session, ordered by (at_us, id))
          reused (outside, truth session) ──▶ session_reused_outside_run (excluded)
      agents_and_sessions(truth) ──▶ agent names (every name any row gives),
                                     claims per session (session rows first)
      index_agents ──▶ AgentIndex: session → owner, exchange → owner;
                       session_conflict (noted) when two agents claim one
      turn_check ──▶ client_turn_mismatch (noted)
      world: decl = truth agents (model, anthropic/claude);
             exchanges = the owned in-window ones, sorted (at_us, agent, id);
             messages = those they use, in capture order;
             exchange_agent rows from the index
      Checker(WorldInputs, exchange_agent rows)
      Resolver::resolve(truth rows) ──▶ labels (each checked), counts, diagnostics
      World::new(dataset, decl, messages, exchanges, rows + labels,
                 complete {construction})
      World::add_note(failure name, count) per diagnostic name ──▶ Labelled
  FilesRead: truth, messages, exchanges

TraceSource::worlds() ──▶ the one World
write_export(inputs, out_dir, options, bench converter)
  DemoSwarmSource::open (above)
  capture_manifest::read(inputs.manifest) ── missing or not a manifest ─▶ CaptureManifest {Read}
  capture_manifest::check: an input view; dataset = truth's; version 1; worlds = [truth world];
                           selection = Options::settings()          ─ else ─▶ CaptureManifest {…}
  Labelling {bench: converter, truth {path as given, BLAKE3 of its bytes}}
  ManifestInfo = the capture's {dataset, version, source, converter, selection, pace}
  corpus::export(source, out_dir, info, Unsplit) ──▶ messages, exchanges, labels,
                                                    manifest (world notes = diagnostics by failure)
  manifest.selection, split = the capture's (drops the split's split_list)
  capture_manifest::differences(manifest.input_view(), capture)
      any ─▶ manifest.json removed, NotTheCapture {top-level paths}
  write_manifest ──▶ manifest.json (input view = the capture's; same digest)
  write_diagnostics(…, labelling) ──▶ diagnostics.json {labelling, report…}

write_holdout_export(inputs, out_dir, options, bench converter, release)
  as write_export, plus: holdout::check_capture (split dev, no release or
  capture_digest in its selection) before anything is written; after the
  input-view comparison, holdout::mark:
    split ─▶ holdout
    selection.release ─▶ release ("<detector>@<version>")
    selection.capture_digest ─▶ the capture manifest's Manifest::digest (hex)
  ─▶ manifest.json; diagnostics.json as for dev

holdout::capture_digest(export manifest)
  not split holdout of demo-swarm/<scenario> ─▶ None
  selection.capture_digest missing or not a digest ─▶ error
  capture_view(export) = input view, split dev, release and capture_digest removed
  its digest ≠ the recorded one ─▶ error; else Some(recorded)
```

### Holdout exports

A demo-swarm holdout is a whole run (a fresh node0 run with a seed of at
least 1,000,000), not a world of a dev list's complement. The adapter
writes the capture as a dev input view and its predictions name that
view's digest; the bench must not rewrite them. So the holdout export's
manifest differs from the capture's only in `split` and the two holdout
selection entries, and records the capture's digest
(`selection.capture_digest`) so the scorer can accept predictions that
name it. That digest is never trusted as written: it must equal the digest
of the capture view rebuilt from the export. A detector run on the
holdout export's own input view (`a2a-bench run`) names the export's
digest, which is accepted as for every export.

### The join (per row)

- **Reader**: session `reader_session`, exchange at ordinal `reader_turn`
  first, then every other exchange of the session in order; the first one
  whose request holds a tool result for the call id (the last such result
  in the request) is the reader exchange. Its text must hash (BLAKE3) to
  `content.blake3`, else `hash_mismatch` and the row is dropped. Found at
  another ordinal: `turn_mismatch`, kept. Not found: `turn_out_of_range`
  when the turn is past the session's exchanges, else `tool_use_missing`;
  dropped. Unknown session: `unknown_session`, dropped. A result without
  text, or with empty text: `body`, dropped.
- **Writer** (transmissions only): session `writer_session`, ordinal
  `writer_turn` first, the same fallback; the response's tool call with
  the call id must be a `PUT` with string `method`, `url` and `body`
  (`not_a_put`) whose `body` hashes to `content.blake3`
  (`hash_mismatch`). Any failure keeps the label without a sender
  exchange (`kept_without_sender`).
- **Location**: the whole text of the reader's tool result part.

### Rows to labels

| Row | Label | id |
| --- | --- | --- |
| `session` | none; maps the session to its agent (`unknown_session` noted when the capture lacks it) | |
| `transmission` | `transmission`: route `channel {url}` (`normalized_url` of the route URL), carrier `tool_result`, tier `construction`, needs `MatchNeed::through_json_string` of the body (`decoded [json_string]` when it holds `"`, `\` or a control character, else `exact`) | `line/<n>` |
| `self_read` | `negative_control` `self_read`, writer → writer, at the read, with its text | `line/<n>` |
| `reread` | `negative_control` `reread`, writer → reader, at the read, with its text | `line/<n>` |
| `miss` | `negative_control` `miss` from every other agent of the world to the reader, at the read (the not-found result), no text; no digest check | `line/<n>/<sender>` |
| `unattributed_read` | `exemption` `unknown_sender` at the read, with its text | `line/<n>` |
| `agent_cluster` | `agent_cluster` with `cluster: key_group` of its distinct agents (sorted), tier `construction`, when two or more; else `key_group_not_a_cluster` (noted) | `line/<n>` |

Every label's source is `{file: <truth file name>, path: "line/<n>"}`.
Coverage is `complete {construction}`.

Each label is checked by `check_labels` beside the world's
`exchange_agent` rows before it is kept. One the format refuses is
`invalid_label` and dropped; a transmission refused only for its sender
exchange is kept without it (`invalid_label`, writer side,
`kept_without_sender`).

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/datasets/demo-swarm/src/lib.rs` | crate root, constants, options | `DATASET_PREFIX`, `HEADLINE`, `BOILERPLATE`, `VERSION`, `MODEL`, `Options` (`margins`, `settings`) |
| `src/schema.rs` | truth v2 rows, strict | `TruthLine`, `Header`, `Scenario` (`dataset`, `dataset_name`), `SessionStart`, `Delivery`, `Miss`, `UnattributedRead`, `KeyGroup`, `TruthRoute`, `TruthCarrier`, `ReadTool`, `Content`, `WireAt`, `HexDigest`, `InvalidDigest`, `VERSION` |
| `src/truth_file.rs` | reading a truth file with line numbers | `read`, `TruthFile`, `Row`, `Numbered`, `DeliveryKind`, `TruthFileError` |
| `src/window.rs` | the run window and the split | `RunWindow` (`of`, `contains`), `Margins`, `DEFAULT_LEAD_MS`, `DEFAULT_SLACK_MS`, `truth_sessions`, `split`, `Split`, `Reused` |
| `src/capture.rs` | reading and checking the adapter's files | `read`, `Capture`, `CaptureError` |
| `src/capture_manifest.rs` | the capture's manifest: read, checked, compared with the export's input view | `CAPTURE_MANIFEST_FILE`, `read`, `check`, `differences`, `CaptureManifestError` |
| `src/sessions.rs` | exchanges by session, ordinals | `Sessions` (`index`, `get`), `Session` (`at`, `ordinal`) |
| `src/locate.rs` | tool results and `PUT` calls in exchanges | `tool_result`, `write_call`, `FoundResult`, `FoundCall`, `LocateError`, `MessageIndex` |
| `src/resolve/mod.rs` | agents, claims, the row loop | `AgentIndex`, `ResolveCounts` |
| `src/resolve/join.rs` | reader and writer joins | (crate-internal) |
| `src/resolve/rows.rs` | rows to checked labels (needs from the format's `MatchNeed::through_json_string`) | |
| `src/resolve/check.rs` | one label against the world | (crate-internal `Checker`) |
| `src/diagnostics.rs` | the typed join-diagnostics table | `Diagnostic`, `Diagnostics` (`table`, `by_failure`, `render`, `named`), `DiagnosticCount`, `JoinFailure`, `RowKind`, `Side`, `Effect` |
| `src/label.rs` | labelling one capture | `label`, `Labelled` (`report`), `CaptureCounts`, `DiagnosticsReport`, `LabelError` |
| `src/source.rs` | trace source, export, diagnostics file | `DemoSwarmSource` (`open`, `labelled`, `into_labelled`, `files_read`, `run`), `Inputs` (`in_dir`; `truth`, `messages`, `exchanges`, `manifest`), `write_export`, `write_holdout_export`, `write_diagnostics`, `Labelling`, `TruthRef`, `read_truth`, `DIAGNOSTICS_FILE`, `Error` |
| `src/holdout.rs` | the holdout marks on a capture's manifest and the capture digest a holdout export's predictions may name | `CAPTURE_DIGEST_KEY`, `check_capture`, `mark`, `is_holdout`, `capture_view`, `capture_digest`, `HoldoutError` |
| `tests/swarm_truth/` | ported ct-eval tests over a synthetic capture (`fixture.rs`; `truth`, `join`, `window`, `sessions`, `scenario`, `export`) | |
| `tests/real_truth.rs` | ignored: row counts of a real truth file (`DEMO_SWARM_TRUTH`) | |

## Invariants and constraints

- **Labels are the truth's.** Nothing a detector computes is used; the
  resource is the bench canonicaliser's `normalized_url`, which is
  parity-tested to equal crosstalk-flow's `url_locator`.
- **The bench only adds truth.** The export's manifest is the capture's
  `manifest.json` plus `files.labels` and each world's `labels` and
  `notes`; its `input_view()` equals the capture's manifest and its
  `digest()` is the one the capture's predictions name (tested; on the
  two saved node0 runs `score` accepts the adapter's predictions). Source,
  converter, selection and pace are the capture's; the bench's own
  version and commit and the truth file's path and BLAKE3 are in
  `diagnostics.json` under `labelling`, never in the manifest.
- **A holdout export is the capture marked, nothing else.** Its input
  view equals the capture's but for `split` (dev → holdout),
  `selection.release` and `selection.capture_digest`; the recorded capture
  digest must be the digest of that view with the marks undone.
- **Exchange ids are the gateway's**, carried; exchanges are copied from
  the capture verbatim (client included). A capture holding exactly the
  run gives back its `messages.jsonl` byte for byte.
- **Nothing fails silently.** Every row that does not join, every label
  the format refuses, every reused-session exchange left out, and every
  `client.turn` that disagrees with the ordinal is one diagnostic with
  its effect (`dropped`, `kept`, `kept_without_sender`, `noted`,
  `excluded`).
- **Manifest notes.** The world's `notes` in `manifest.json` are the
  diagnostics counted by failure name (`key_group_not_a_cluster`,
  `hash_mismatch`, …), so they sum to `diagnostics.json`'s entry count
  (tested). Truth: the input view drops them.
- **The world is assembled with `World::new`, not the builder.** The
  capture's exchanges (ids, clients, `response.error`, messages) are
  carried verbatim; the builder's `recorded_exchange_with_client` keeps
  the client but would drop a failed call's `response.error` (the builder
  writes none), fold free-text stop reasons into `StopReason` and rebuild
  message storage, so it is not used.
- **One run is one world.** The world holds the in-window exchanges of
  the sessions the truth names; agents are every name the truth gives.
- **Determinism.** Agents, sessions and claims are ordered maps;
  exchanges sort by (time, agent, id); two exports of the same inputs are
  byte-identical (tested), `diagnostics.json` included.
- **No excerpts.** The truth's `content.excerpt` is parsed (strict
  schema) and never copied into labels, diagnostics or docs.
- The format's world checks apply: an agent's exchanges must strictly
  increase in time. Two exchanges of one agent at the same microsecond
  (two sessions at once) fail the world with `LabelError::World`;
  ct-eval had no such rule.

## Known ct-eval quirks kept

- Agents are every name any row gives, including agents named only by a
  key group or a `session` row; a `miss` yields a control from each of
  them, exchanges or not.
- A session's owner is the alphabetically first agent of its `session`
  rows, else of the other rows naming it, not the first in file order.
- The reader is the *last* matching tool result in the request; the
  fallback exchange is the first of the session (in ordinal order) that
  holds one, after the one at the named turn.
- A reader hash mismatch drops the row; a writer hash mismatch keeps it
  without a sender. A miss is joined with no digest check.
- `turn_out_of_range` versus `tool_use_missing` depends only on whether
  the named turn is past the session's exchange count.
- `needs` (the format's `MatchNeed::through_json_string`) is `decoded [json_string]` whenever the body holds a character
  JSON escapes, since the writer's `PUT` carried it escaped, even though
  the reader's tool result is raw.
- The window's latest time is the greatest `at_unix_ms`,
  `read_at_unix_ms` or `written_at_unix_ms` of any row; `session` and
  `agent_cluster` rows are untimed. A truth with no timed row ends at its
  start plus the slack.

## Differences from ct-eval (intended)

| ct-eval | bench | why |
| --- | --- | --- |
| reads the gateway's exchange log and blob store | reads the adapter's bench capture | gateway formats stay in crosstalk (design §5.3) |
| key groups never labelled (`key_group_not_a_cluster`, noted, for every group) | groups of two or more distinct agents are `agent_cluster` rows of kind `key_group`; smaller ones noted as before | the format has a key-group cluster kind (design §3.5) |
| a label whose reader exchange is another agent's (after a `session_conflict`) is kept | dropped as `invalid_label` | the format's `check_labels` refuses it (`WrongAgent`) |
| a sender exchange that is another agent's or not before the read is kept | the label is kept without it (`invalid_label`, writer side) | same |
| the world holds labels only; the traffic count is set by hand | the world holds the run's exchanges | an export carries its inputs |
| labels have no id | `line/<n>`, misses `line/<n>/<sender>` | the format requires unique ids |
| `Locator::Url {scheme, host, path, query}` | `Resource::Url(<canonical text>)` | the bench's resource shape; same canonical text |
| — | `client_turn_mismatch` (noted) | cross-checks the adapter's `client.turn` |
| detection diagnostics in the same table | not here | the adapter's and the scorer's |
| `Effect::PredictionsDropped` | removed | detection only |

## Real-data comparison

The saved node0 runs (`crosstalk/bench-runs/20261006T020835Z`, headline,
and `20261006T021639Z`, boilerplate) hold the gateway's own formats;
the adapter that writes their captures is being built, so the full
label comparison waits for it (stage P7). Done now: both runs'
`truth.jsonl` parse under the strict v2 schema, and their row counts
(`tests/real_truth.rs`) against ct-eval's `diagnostics.json` from the
parity baseline (`parity-out/bench-*-swarm/`, ct-eval at 7f8a2fb, every
row joined, nothing dropped):

| count | headline: bench | headline: ct-eval | boilerplate: bench | boilerplate: ct-eval |
| --- | ---: | ---: | ---: | ---: |
| rows after the header | 108 | 108 | 108 | 108 |
| `session` | 29 | 29 | 29 | 29 |
| `transmission` | 55 | 55 | 50 | 50 |
| `self_read` | 0 | 0 | 0 | 0 |
| `reread` | 0 | 0 | 2 | 2 |
| `miss` | 4 | 4 | 7 | 7 |
| miss controls (misses × 19 other agents) | 76 | 76 | 133 | 133 |
| `unattributed_read` | 0 | 0 | 0 | 0 |
| `agent_cluster` (key groups) | 20 | 20 | 20 | 20 |
| key groups of two or more agents | 0 | — | 0 | — |
| agents | 20 | 20 | 20 | 20 |
| run window start (Unix ms) | 1791252511600 | 1791252511600 | 1791252995338 | 1791252995338 |
| run window end (Unix ms) | 1791252702413 | 1791252702413 | 1791253184755 | 1791253184755 |

ct-eval's tables for both runs hold only `agent_cluster / row /
key_group_not_a_cluster / noted / 20`; every key group is one agent
(`agents_per_key` 1), so the bench reports the same 20 and labels no
cluster. ct-eval resolved 254 (headline) and 247 (boilerplate) in-window
exchanges, none excluded; the bench's world should hold the same counts
once the capture exists.
