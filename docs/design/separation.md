# Design: a2a-transmission-bench, split from crosstalk-eval

Status: **approved 2026-10-05** (with the decisions in §10). Porting
starts with the format crate.
Source: crosstalk `crates/eval` (`crosstalk-eval`, binary `ct-eval`) on
`origin/staging` at `bc37b2d`, which already holds #98's single dismissed
rule (merged in `be41206`).

## 1. Goal

A neutral benchmark for one task: **given labelled multi-agent LLM traces,
find the agent-to-agent transmissions in them.** Any detector can be
scored. crosstalk is one detector, plugged in through an adapter that
lives in crosstalk.

Why split:

- **Credibility.** The detector should not grade itself. Today the
  scorer, the labels and the gates are in the detector's workspace, and
  the AI Village labels are produced with the detector's own extractor.
- **Reuse.** The converters, labels and scorer are useful to any
  transmission detector.
- **Gates out of the detector's repo.** Regression gates become the
  bench's per-detector baselines, not something the detector edits next
  to its own code.

### Non-goals

- Detection. The bench keeps a naive reference matcher as "baseline
  detector 0", and it stays naive.
- Hosting dataset bytes. None are committed; datasets are read from a
  root (default `~/Data/ai/agents`).
- Identity resolution and channel discovery scoring. `agent_cluster`
  labels are carried for later; nothing scores them yet.
- Live/online scoring against a running gateway. The bench scores files.
  Driving a live system (crosstalk's node0 bench) is the detector's
  business, which then hands the bench files.

## 2. Shape of the bench

```text
dataset files ─▶ a2a-bench export ─▶ export dir (format v1)
                                     ├─ manifest.json
                                     ├─ messages.jsonl      (inputs: what a detector may read)
                                     ├─ exchanges.jsonl     (inputs)
                                     └─ labels.jsonl        (truth: never shown to a detector)
                                                │
       input view (manifest + messages + exchanges) │
                                                ▼
                        detector process (any language)
                        e.g. ct-bench-detect (crosstalk), a2a-reference
                                                │
                                                ▼
                                     predictions.jsonl
                                                │
labels + exchanges + predictions ─▶ a2a-bench score ─▶ report.json, table, gate outcomes (exit 2 on failure)
```

The three stages only meet through files, so each one can be replaced,
rerun and diffed on its own. `a2a-bench run` chains them for
convenience.

### Workspace (Rust, edition 2024)

| Crate | Role | Depends on |
| --- | --- | --- |
| `a2a-bench-format` | the on-disk types (§3), their checked constructors, JSONL readers and writers, version handling, canonical JSON, BLAKE3 ids | serde, serde_json, blake3, thiserror |
| `a2a-bench-resource` | the neutral resource canonicaliser (§6.1): URLs, forge repositories, repo files, threads | format, url |
| `a2a-bench-score` | alignment, judge, scorer, report, gates (moved from `score/`, `report/`) | format |
| `a2a-bench-reference` | the reference matcher (moved from `reference/`), as a library and as the `a2a-reference` detector binary | format |
| `a2a-bench-datasets` | the converters (one module tree per dataset), the corpus builder and pace clock (moved from `corpus/`, `datasets/`) | format, resource, parquet, flate2 |
| `a2a-bench-cli` | the `a2a-bench` binary: `export`, `validate`, `run`, `score`, `diff` | all of the above, clap, tracing |

No crate depends on any crosstalk crate. crosstalk depends on
`a2a-bench-format` (a git dependency pinned to a bench tag) for its
adapter.

## 3. On-disk format v1

The normative format is [../features/format.md](../features/format.md)
and the `a2a-bench-format` crate. Building it changed a few details below:
every file is split into per-world sections (messages de-duplicated within
a world), a message nests its role and parts under `body`, a tool result
has no tool name (the spec's has none), coverage is on the labels world
row, and every file has a header and a trailer whose digest covers it.

All files are UTF-8 JSONL, one JSON object per line, each tagged by
`kind`. Readers reject unknown kinds and unknown fields
(`deny_unknown_fields`), and every file starts with a `header` line that
names the format version (§8). Maps that reach output are ordered, so an
export, a predictions file and a report are byte-identical across reruns.

### 3.1 Identifiers

| Id | Form | Derivation |
| --- | --- | --- |
| `DatasetId` | `salt`, `ai-village`, `demo-swarm/headline`, … | fixed per converter |
| `WorldKey` | string, unique in a dataset | the converter's (e.g. SALT trace file stem) |
| `AgentKey` | string, unique in a world | the converter's |
| `ExchangeId` | 26-char Crockford ULID | **unchanged from ct-eval**: BLAKE3 over dataset id + `SourceRef`, with the exchange's virtual time in the ULID time bits. Recorded ids (demo-swarm, the gateway's minted ULIDs) are carried, not re-derived |
| `MessageId` | 64 hex chars | BLAKE3 of the message's canonical JSON (§3.3) under the context `a2a-bench/v1 message` |
| `SourceRef` | `{file, path}` | file relative to the dataset root, JSON-pointer-like path inside it |

Keeping ct-eval's exchange id derivation means a crosstalk run through
the bench ingests the same ids it ingests today, so ct-eval and bench
outputs can be diffed id by id during parity (§7).

### 3.2 `manifest.json`

```json
{
  "format": "a2a-bench/1",
  "dataset": "salt",
  "dataset_version": "salt@2",
  "source": {"root_relative": "salt-nlp", "revision": "<HF snapshot hash or git HEAD>", "digest": "<BLAKE3 over the files read, sorted by path>"},
  "converter": {"crate_version": "0.1.0", "git": "<bench commit>"},
  "selection": {"limit": 53, "stratified": true},
  "pace": {"seed": 0, "min_ms": 1000, "max_ms": 5000},
  "worlds": [{"key": "…", "exchanges": 812, "labels": 77, "coverage": {"complete": {"tier": "construction"}}}],
  "digests": {"messages.jsonl": "…", "exchanges.jsonl": "…", "labels.jsonl": "…"}
}
```

The manifest pins everything that changes the bytes: source revision,
selection, pace. Two exports with equal manifests are byte-identical
(tested). `coverage` per world is `complete {tier}` or `partial` (§5).

### 3.3 `messages.jsonl`: provider-neutral messages

Messages are stored once and referenced by id, because requests carry
the whole history and repeat most messages (SALT, AI Village).

```json
{"kind":"message","id":"…","role":"assistant","parts":[
  {"type":"text","text":"I'll post the summary."},
  {"type":"reasoning","text":"…"},
  {"type":"reasoning_opaque"},
  {"type":"tool_call","call_id":"call_1","name":"http_request","arguments":{"json":"{\"body\":\"…\",\"method\":\"POST\",\"url\":\"…\"}"}},
  {"type":"media","media_type":"image/png"}
]}
{"kind":"message","id":"…","role":"tool","parts":[
  {"type":"tool_result","call_id":"call_1","name":"http_request","is_error":false,"content":[{"type":"text","text":"…"},{"type":"media","media_type":"image/png"}]}
]}
```

- `role`: `system | user | assistant | tool`. Part types per role mirror
  what providers carry (system/user: text, media; assistant: text,
  reasoning, reasoning_opaque, tool_call, server_tool_result; tool:
  tool_result). `unknown` holds a provider block the converter could not
  map (no text).
- **Part text is the contract.** A location's byte range indexes the
  part's text, defined in exactly one place
  (`a2a_bench_format::part_text`) and pinned by the format version:

  | Part | Text |
  | --- | --- |
  | `text` | `text` |
  | `reasoning` | `text` (never a signature) |
  | `tool_call` | `arguments.json` (canonical JSON) or `arguments.invalid` (verbatim) |
  | `tool_result`, `server_tool_result` | its `text` contents, in order, joined with `"\n"` |
  | `reasoning_opaque`, `media`, `unknown`, a result without text | none |

  This is crosstalk-spec's `Message::part_text` table today. Opaque
  provider material is never part text and is not even stored: call ids
  stay (they pair calls and results) but are excluded from text,
  signatures and encrypted reasoning are dropped to `reasoning_opaque`.
  Visible reasoning that carries a signature (SALT's signed
  `thinking_blocks`) is `reasoning` with its text, signature dropped; it
  is not `reasoning_opaque`.
- **Normalisation rules every converter follows**, because part indices
  are only comparable if messages split identically:
  - tool results are their own `tool`-role messages, one per result
    block, in order; a provider user message that mixes tool results and
    text becomes the tool messages first, then a user message with the
    rest (crosstalk's Anthropic normaliser does exactly this, and spec
    `UserPart` has no tool result);
  - system content stays **inline, in its original position** in the
    request's message list, never hoisted (Claude Code reminders and
    crosstalk's `--claude-code-shape` send mid-array system turns, and
    demo-swarm truth indexes the wire array including them);
  - message boundaries and part order are the provider's, so a
    location translates to and from crosstalk's `PartRef` mechanically.
- `unknown` keeps no raw bytes. A detector cannot see unknown blocks,
  as the gateway cannot use them either.
- **Canonical JSON** is RFC 8785 with exact numbers (a number keeps its
  decimal digits instead of going through an IEEE double), the same rule
  as crosstalk-spec's `CanonicalJson`. Tool-call arguments are stored as
  that text, so byte offsets into them are stable. The bench carries its
  own implementation and its own test vectors; parity checks it against
  the spec's (§7).
- `MessageId` hashes the canonical JSON of the message object with `id`
  removed.

### 3.4 `exchanges.jsonl`: one model call each

```json
{"kind":"world","key":"trace-0007","agents":[{"key":"alice","driven":"model","model":"gpt-4o"},{"key":"bob","driven":"scripted"}]}
{"kind":"exchange","id":"01J…","world":"trace-0007","at_us":1767225601000000,
 "client":{"credential":"k:3f2a…","session":null,"turn":null,"vendor":"openai","model":"gpt-4o"},
 "request":{"messages":["<mid system>","<mid>","<mid>"],"tools":[{"name":"http_request","description":"…","schema":{…}}]},
 "response":{"messages":["<mid>"],"stop":"tool_use"},
 "fidelity":"exact","source":{"file":"…","path":"/episodes/2/messages/14"}}
```

- `world` rows declare agents first; exchanges follow in time order
  (`at_us`, microseconds since the Unix epoch, strictly increasing per
  agent; a sender's exchange precedes the reader's). A derived
  `ExchangeId`'s ULID time is `at_us / 1000` (milliseconds); the pace's
  1 s minimum step keeps that collision-free.
- `request.messages` is one ordered list, system messages inline (§3.3).
- `request.tools` is optional and absent when the dataset does not
  record schemas (SALT, AgentDojo); converters never invent them.
- `client` is **only what a proxy in front of the model would observe**:
  an opaque stable credential fingerprint, a session/conversation header
  if the dataset has one and the request's 0-based ordinal in it
  (`turn`; demo-swarm joins on session + turn), vendor and model. It is
  not the agent. Several
  agents may share a credential (demo-swarm key groups), so detectors
  must attribute exchanges themselves, as a gateway does.
- The true agent of each exchange is in `labels.jsonl` (`exchange_agent`
  rows), not here. The world's agent list (names, driven or scripted) is
  public, as it is in every dataset.
- `fidelity`: `exact | reconstructed | synthetic`, as today.

### 3.5 `labels.jsonl`: truth

One file per export, never handed to a detector. Rows, in world order:

| `kind` | Fields | From ct-eval |
| --- | --- | --- |
| `exchange_agent` | `exchange`, `agent` | the `World`'s exchange→agent map |
| `transmission` | `id`, `from`, `to` (agents), `reader_exchange`, `read_at` (location), `route` (`direct`, `channel {resource}`, `delegation {direction}`), `carrier` (`tool_result`, `user_turn`, `system_prompt`, `reader_output`), `content` (`text`, optional `origin_at`), `needs` (§3.7), `tier` | `ExpectedTransmission` |
| `access_only` | as `transmission` minus `content`/`needs`, route must be `channel` | `ExpectedAccess` |
| `negative_control` | `id`, `reason` (`rejected_send`, `shared_source`, `boilerplate`, `no_sender_exchange`, `self_read`, `reread`, `miss`), and at least one bound: `reader_exchange`, `read_at`, `origin_at` | `NegativeControl` |
| `exemption` | `id`, `reason`, `reader_exchange`, `read_at` | `Exemption` |
| `agent_cluster` | `id`, `agents`, `kind` (`key_group`, `identity`) | `AgentCluster` |

A **location** is `{"exchange": id, "message": mid, "part": u16, "start": u32, "end": u32}`,
the byte range over the part's text (§3.3). `message` + `part` are what
crosstalk calls a `PartRef`, with the bench's `MessageId` in place of the
spec's `MessageHash`.

**Tiers**: `construction`, `structural`, `heuristic`, `judged`,
`out_of_reach`, `forwarding`, with today's meaning: the last two are
reported apart and never move `overall` or access-only recall.

An `exemption` is scoped to its reader exchange and location, never to
the whole world.

Every label row is built through checked constructors, and read back
through the same checks: sender ≠ reader, both in the world; content text
equals the text at `read_at`; a control is bounded; an access-only label
is on a channel; `needs` agrees with `tier` (`MatchNeed::out_of_reach`).

### 3.6 `predictions.jsonl`: what a detector writes

```json
{"kind":"header","format":"a2a-bench/1","detector":{"name":"crosstalk-live","version":"<git sha>","variant":"forwarding-off","config_digest":"…"},"export":{"dataset":"salt","manifest_digest":"…"}}
{"kind":"world","key":"trace-0007","status":"scored"}
{"kind":"attribution","world":"trace-0007","agent":"d:01J…","exchanges":["01J…","01J…"]}
{"kind":"transmission","world":"trace-0007","id":"t:…","state":"confirmed","quality":{"class":"exact","carrier":"user_turn"},
 "evidence":[
   {"type":"content","from":"d:01J…","reader_exchange":"01J…","read_at":{…location…},"origin_at":{…location…},"class":"decoded","codecs":["json_string"],"carrier":"user_turn","route":"direct"},
   {"type":"co_access","from":"d:01J…","write_exchange":"01J…","write_at":{…},"reader_exchange":"01J…","read_at":{…},"resource":{…}}
 ]}
{"kind":"trailer","worlds":1,"transmissions":1,"digest":"…"}
```

- `attribution` rows give the detector's agents as sets of exchanges.
  The scorer maps each detector agent to the true agent of its exchanges.
  A split (several detector agents for one true agent) is fine; a merge
  (one detector agent over two true agents) fails the world with a typed
  failure naming the ids, exactly as `AgentMapError::Merged` does today.
- Detector agent ids must be canonical, resolved through any merges at
  write time, so attribution rows hold no aliases.
- Attribution may be partial. A detector that cannot tie a sender to
  exchanges writes `{"kind":"unattributed","world":…,"agent":…}`. The
  scorer then reports that agent's predictions as `unknown_detected_agent`
  (today's swarm diagnostic) instead of failing the world. This is the
  gateway-export path's case: a `ContentMatch` names `origin_agent` but
  not an origin exchange, so a sender that is never a reader or accessor
  cannot be placed over HTTP today. Channel routes are covered by the
  write access's exchange; direct and unobserved routes are not.
  crosstalk-side fix, agreed with crosstalk-impl (spec on
  `feat/conversation-reads`, served from its stage B):
  `POST /query/exchange-turns` → `ExchangePlacement { agent, conversation,
  turn }` per exchange (agent canonical at read time; unknown and
  never-threaded exchanges left out), which gives the attribution rows,
  and `POST /query/span-points` → `SpanPoint { agent, exchange, location,
  … }` per origin span, which places senders on direct and unobserved
  routes and gives the export path its `origin_at`. Until those are
  served, the from-export adapter writes `unattributed` rows.
- A `transmission` has a `state`: `confirmed | classified | aggregated`
  (content evidence), `suspected | discarded` (co-access only). It
  becomes one prediction per content evidence, or per co-access, as
  `predict::from_transmission` does. `detected`/`awaiting_content` rows
  are allowed and make no prediction.
- Content evidence carries the reader location (required) and the
  origin location (optional; the gateway export path does not know it,
  so no control that names an origin applies there, as today).
- `route.channel` and `co_access.resource` are a bench `Resource` (§6.1),
  which the scorer canonicalises again before comparing. A detector that
  writes a raw URL gets it canonicalised by the bench, not by itself.
- `world.status` is `scored`, `no_consumers` (crosstalk's
  `PipelineDetector`: counted unscored, as today) or `failed {reason}`.
- The trailer's digest makes a truncated file an error, not a smaller
  score.

The file is everything the scorer needs from a detector. There are no
read traits across the boundary: the crosstalk adapter resolves spans,
accesses and channels on its side (`Resolved::gather` stays in crosstalk)
and writes them out as locations and resources.

### 3.7 Match needs and evidence classes

`needs` keeps ct-eval's `MatchNeed`: `exact`, `normalized`,
`decoded {codecs}` (`json_string`, `yaml_string`, `base64`, `hex`,
`url`, `unicode`), `undecodable {codec}` and `unobserved` (both imply
`out_of_reach`), `semantic`. Prediction classes are `exact`,
`normalized`, `decoded {codecs}`, `semantic`, `suspected`, `discarded`.
Class and carrier pick the report row; they never decide alignment.

## 4. The detector contract

A detector is a program that turns an export's input view into a
predictions file:

```text
<detector> --input <dir> --output <predictions.jsonl> [detector-own flags]
```

- `<dir>` holds `manifest.json` (with `worlds[].labels` removed),
  `messages.jsonl` and `exchanges.jsonl`. `a2a-bench run` builds it with
  hard links or copies, never with `labels.jsonl` in it, so a detector
  cannot read the truth by accident. (`exchange_agent` rows are truth too,
  which is why they are not in `exchanges.jsonl`.)
- Worlds are independent. A detector must not carry state between worlds
  (resources canonicalise by URL, so a shared store cross-links worlds;
  crosstalk builds a fresh composition per world today for this reason).
- Exit 0 with a complete file (trailer present) is a run. A world the
  detector could not process is a `world` row with `failed`, and the run
  goes on; exit non-zero is a run failure.
- Detector settings (windows, forwarding, extract configs) are the
  detector's flags. Their digest and a short `variant` name go in the
  header, and gates select on `detector.name` + `variant`.
- Determinism is the detector's responsibility; the bench reports two
  runs' digests if asked (`a2a-bench run --twice`).

The reference matcher ships as `a2a-reference`, the first detector to
satisfy the contract, and is the bench's own test of it.

### 4.1 How crosstalk satisfies it

The adapter is a new crosstalk binary, `ct-bench-detect`, in what is left
of `crates/eval` (renamed `crosstalk-bench-adapter`):

```text
read manifest, messages, exchanges (a2a-bench-format)
per world:
  convert each bench message to a spec Message
    check: spec part_text(part) == bench part_text(part) for every part, else fail the world
    keep MessageId <-> MessageHash both ways
  convert each exchange to a checked NormalizedExchange
    id = bench ExchangeId (same ULID), IngressMode::Replay { corpus: eval-<dataset> },
    synthetic ClientContext from client.credential (as corpus::client does now)
  LiveDetector<GatewayBackend> (unchanged): build, ingest at at_us, settle, list transmissions, attribution
  Resolved::gather (unchanged) -> locations and resources
  write attribution + transmission rows, translating MessageHash -> MessageId and Locator -> bench Resource
write header and trailer
```

- `--detector pipeline` becomes `ct-bench-detect --mode pipeline` and
  writes `status: no_consumers` worlds.
- The node0 bench (`run.sh bench`, `ct-eval swarm-fetch`) stays in
  crosstalk. A new `ct-bench-detect from-export --run <dir>` turns the
  saved `export.jsonl` + `evidence.jsonl` + exchange log + blobs into
  bench `exchanges.jsonl`/`messages.jsonl` and `predictions.jsonl`,
  carrying the gateway's minted exchange ids; the bench's demo-swarm
  converter labels them from truth v2 (§5.3). `ct-eval replay` becomes
  `ct-bench-detect replay`, same inputs, Live in memory, bench output.
- The adapter keeps crosstalk's tests that are about crosstalk
  (`live_gateway.rs` byte-identical reruns, `tests/live.rs` over a
  scripted backend, `pipeline.rs`) and gains one: bench part text equals
  spec part text over every fixture.

## 5. What moves, what stays

### 5.1 Inventory

Measured on `bc37b2d` (`src/` 26.9k lines, `tests/` 14.1k):

| ct-eval module | Lines | Goes to | Notes |
| --- | --- | --- | --- |
| `score/` (align, judge, scorer, sources, quality) | 996 | bench `score` | `quality.rs` bridges to the spec's `DetectionQuality`: that cross-check moves to the adapter's tests (it is a spec type); the bench keeps transmission rows keyed by `quality` |
| `truth/` | 645 | bench `format` | spec types replaced by bench types (§3) |
| `report/` (table, gates, `GateSearch`) | 826 | bench `score` | `CT_EVAL_GATES` → `A2A_BENCH_GATES`; the installed path goes |
| `reference/` | 1,561 | bench `reference` | becomes `a2a-reference` |
| `predict/` | 778 | split | `Prediction`, `AgentMap` → bench; `from_transmission`, `reads.rs`, `memory.rs` (spec read traits) → adapter |
| `corpus/` (builder, clock, client, delta, exchange) | 853 | bench `datasets` | `client.rs` (synthetic `ClientContext`) → adapter; the bench keeps only the credential fingerprint |
| `keys.rs`, `ids.rs`, `location.rs`, `config.rs` | 473 | bench | id derivation kept bit-for-bit |
| `datasets/` salt, agentdojo, tau2, wiki, swarm, open_swe, lmcache, swe_splice, cipher, chat, background, parquet_rows, rng | 9,428 | bench `datasets` | spec types only today: a mechanical port onto bench types |
| `datasets/ai_village` | 5,095 | bench `datasets` | un-coupled, §5.2 |
| `datasets/swarm_truth` schema, truth_file, window, resolve, locate, diagnostics | 1,872 | bench `datasets` | `resolve.rs`'s `url_locator` → the bench canonicaliser; inputs become bench files (§5.3) |
| `datasets/swarm_truth` bodies, exchange_log, detected, fetch, replay, mod (options) | 1,944 | adapter | gateway formats (FsBlobStore, exchange log, exports, evidence) and Live |
| `detect/`, `pipeline.rs`, `gateway.rs` | 1,091 | adapter | unchanged inside; `pipeline::run`'s loop is replaced by the bench runner |
| `bin/ct-eval` (main, swarm, replay) | 1,074 | split | `run`/`truth`/`swarm` scoring → `a2a-bench`; `replay`, `swarm-fetch`, `--detector live/pipeline` → `ct-bench-detect` |
| `gates.toml` | 33 gates | bench `gates/` | one file per detector (§5.4) |
| `datasets.toml`, `extract/agentdojo.json` | | bench / adapter | the AgentDojo extract config is a crosstalk detector flag |
| `tests/` | 14,144 | follow their code | fixtures are synthetic and move with their converter |

About 22k lines of `src/` move to the bench and about 5k stay in crosstalk.

### 5.2 Un-coupling AI Village

Today `datasets/ai_village/{resource.rs, access/}` labels repository,
file and thread accesses by running crosstalk-flow's `ToolExtractors` and
`SitesConfig` over each bash call, so labels match L5 by construction.
That is circular: the bench cannot catch an extractor bug, only what
happens after extraction. Known L5 behaviours it currently hides (from
crosstalk-rollouts): a stale remote binding overriding a `To <url>` line,
and a failed `cd … &&` chain still recorded as a read.

The fix has two parts:

1. **A neutral canonicaliser and command table, owned by the bench**
   (`a2a-bench-resource` plus `datasets::ai_village::shell`). Their spec
   is a table in the bench's docs, not code borrowed from a detector:
   - resources: `repository {host, owner, name}` (lower case, no `.git`,
     nested GitLab groups joined with `/`; remotes in https, scp-like and
     `ssh://` form, `codeload`, the REST API, web `tree/`, Pages
     `o.github.io/n` → `o/n` and bare `o.github.io` → `o/o.github.io`),
     `repo_file {repository, path}`, `thread {repository, kind, number}`
     (GitHub issues/pulls, GitLab issues/merge requests, web or API),
     `collection {repository, kind}`, and `url` (scheme and host lower
     case, default port, fragment and credentials dropped, query sorted);
   - operations per command: `git push` write with unseen payload;
     `git pull/fetch/clone`, `gh repo clone` read; `gh`/`glab`
     `create/comment/note/edit/review` write; `view/list` read;
     `gh api`, `glab api`, `curl`, `wget` by HTTP method;
     `cat/head/tail/sed -n` read; `>`, `>>`, `tee`, here-documents write;
   - write outcome from output (git, curl/wget, gh/glab failures →
     rejected);
   - shell state the converter already owns: persistent cwd, `~`/`$HOME`
     = `/home/computeruse`, clone bindings learnt from push/pull output,
     a failed `&&` chain stops the chain.

   This is truth about what the shell did, derived from the commands and
   their recorded output, not from any detector.

2. **An agreement report, not a label source.** The crosstalk adapter
   (it has `ToolExtractors`) can emit, per AI Village tool call, what L5
   extracted. `a2a-bench diff --agreement` compares that with the bench's
   accesses and lists disagreements by kind. Disagreements are findings
   for crosstalk (or the bench), not silent label changes.

Labels are produced in two versions (§8). `ai-village@1` ports L5's
behaviour as it is at a pinned crosstalk-flow commit, so the first export
reproduces #100's labels exactly (parity). That is more than tables,
because #100 drives crosstalk-flow's stateful `ConversationContext` call
by call (`access::Shell::accesses`). The port covers:

1. the command → operation and locator tables, with `SitesConfig`'s
   forge and Pages hosts;
2. `ConversationContext::observe`'s state transitions: cwd tracking,
   clone and checkout binding directories to repositories
   (`repos().locate(dir)`), and resolving relative paths against the cwd
   and bound repositories (this is where hidden heuristics live, e.g.
   `cd` into an unknown directory);
3. #100's own additions: one persistent shell per agent started at
   `HOME`, `~` expansion, and printed-remote binding (`To`/`From
   <remote>`) followed by re-extracting the same command;
4. `payload::authored`, which decides which typed text is a write's
   content;
5. the `kind()` filter, which keeps only shared resources.

`@1` is done when the agreement report (below) over the full week shows
no differences, or only a short adjudicated list.
`ai-village@2` `ai-village@2` then adds the shell fixes the
neutral table implies (failed `&&` chains, `To <url>` precedence). The
number for crosstalk is expected to move between them; that move is the
point of the split, and is reported, not hidden.

### 5.3 demo-swarm

The truth (`truth.jsonl`, v2) is written by crosstalk's demo swarm, a
traffic generator, not the detector, so it is a legitimate label source.
The converter (schema, run window, session+turn join, tool_use_id and
BLAKE3 cross-checks) moves to the bench. Its inputs change: instead of the
gateway's exchange log and blob directory it reads bench `exchanges.jsonl`
and `messages.jsonl`, which `ct-bench-detect from-export` writes with the
gateway's minted ids and the `session` and turn ordinal in `client`. The
URL canonicalisation in `resolve.rs` uses the bench canonicaliser.

### 5.4 Gates

`gates.toml` moves to `gates/<detector>.toml` in the bench:
`gates/reference.toml` (today's detector-less gates),
`gates/crosstalk-live.toml` (`detector = "live"`; today's
`forwarding = "on"` selector, #99, becomes the `variant` selector, so
forwarding-on gates never apply to a forwarding-off run and the reverse), `gates/crosstalk-gateway-export.toml` (demo-swarm
headline and boilerplate). Semantics are unchanged (recall/precision
`min`, violations/`fp_per_1k` `max`, a gate on a dataset the run did not
score is skipped). A gate change is a bench PR, reviewed in the bench.

## 6. Neutral rules the bench owns

### 6.1 Resources

```json
{"repository":{"host":"github.com","owner":"org","name":"repo"}}
{"repo_file":{"repository":{…},"path":"src/lib.rs"}}
{"thread":{"repository":{…},"kind":"issue","number":12}}
{"collection":{"repository":{…},"kind":"pulls"}}
{"url":"https://example.com/a?b=1&c=2"}
{"file":{"path":"/shared/notes.md"}}
{"opaque":{"tool":"memory_write","key":"…"}}
```

Alignment for channel labels compares canonical resources for equality,
after the bench canonicalises both sides. crosstalk's adapter maps spec
`Locator`s onto these (`Locator::Repository` → `repository`,
`File { host: "<h>/<o>/<n>" }` → `repo_file`, an issue URL → `thread`,
`Url` → `url`, `Mcp`/`Opaque` → `opaque`).

### 6.2 Alignment and judging (unchanged rules)

A prediction and a label align when: same sender and reader (after
attribution), same reader exchange, overlapping reader locations (same
message and part, at least one shared byte), and for a channel label the
same canonical resource. Then, as today: one prediction may find several
labels; only content finds a label; access-only labels are found only by
suspected/discarded predictions; a discarded prediction aligned with
nothing is dismissed; only content violates a control; exemptions, then
controls (most specific first), then coverage decide the rest. The
rules move verbatim with their tests (`score.rs`, `score_many_labels.rs`,
`score_discarded.rs`, `access_only.rs`, `forwarding.rs`).

## 7. Parity plan

Parity means: crosstalk scored through the bench reproduces ct-eval's
numbers on the same build, data and selections. It is proven in stages,
each a diff that must be empty, so a failure points at one layer.

### 7.0 Pin the baseline first

The numbers in the brief come from different builds, and one has not been
reproduced: SALT 0.854/0.955 is crosstalk-impl's measurement on
`fix/l4-nearer-source`; crosstalk-rollouts' last confirmed SALT figure is
0.856/0.810 at `c3cd7f2`. So before any porting:

1. Pin the crosstalk staging commit that includes #105 (gate
   calibration, the cherry-pick of `8708a20`); #98 is already there.
   crosstalk-integration names the commit when #105 lands. Release
   build.
2. Run ct-eval at it with the documented selections and record every
   `report.json`, plus the dataset revisions (HF snapshot hashes, git
   HEADs), in the bench repo as `parity/baseline-<sha>/` (reports only;
   no dataset bytes):

   | Run | Command (ct-eval) | Brief's number |
   | --- | --- | --- |
   | SALT live | `run --dataset salt --detector live --limit 53` (forwarding off) | 0.854 / 0.955 (to re-measure) |
   | wiki live | `run --dataset wiki --detector live --max-agents 100`, and `--demo` | 0.950 (0.960 after e3e99b5; `--demo` 0.985) |
   | swarm-traces live | `run --dataset swarm --detector live` | 1.000 |
   | AI Village live | `run --dataset ai-village --detector live` (Claude Code mode, 993 contexts) | 0.998 (15,772/15,798) |
   | demo-swarm headline | `swarm …` on `bench-runs/20261006T020835Z` | 1.000 / 1.000 |
   | demo-swarm boilerplate | `swarm …` on `bench-runs/20261006T021639Z` | 1.000 / 0.883, 89.1 FP/1k |
   | the rest | AgentDojo, τ², splice `--count 80`, cipher `--count 50`, open-swe/lmcache `--count 16`, every one with `--detector reference` too | as recorded |

   Whatever these runs say is the baseline; the brief's numbers are
   reconciled against them in the doc, not targeted.

### 7.1 Golden export from ct-eval (crosstalk side, small)

Add `ct-eval export --format a2a-bench/1` to crosstalk's current crate:
it writes today's converters' worlds and labels, and `ct-eval run
--predictions-out` writes today's predictions, in the bench format. This
is the only new code in crosstalk before the split, and it is a pure
serialisation of existing types. crosstalk-rollouts takes it once
`a2a-bench-format` has a first commit to pin. It translates spec
`PartRef { MessageHash, index }` to bench `{ MessageId, part }`, which
is mechanical because the bench keeps the spec's message boundaries and
part order (§3.3). It turns every later stage into a byte diff.

### 7.2 Stages

| Stage | Check | Must be |
| --- | --- | --- |
| P1 format | bench canonical JSON and part text vs spec's, over every message of every golden export | equal |
| P2 scorer | `a2a-bench score` on golden labels + golden predictions vs ct-eval's `report.json` | equal counts in every row, equal gates outcome |
| P3 reference | `a2a-reference` on the golden input view vs golden reference predictions | byte-identical predictions |
| P4 converters | `a2a-bench export` vs golden export, per dataset (AI Village at `@1`) | byte-identical files |
| P5 adapter | `ct-bench-detect` on the bench export vs golden live predictions | byte-identical predictions |
| P6 end to end | `a2a-bench run` with `ct-bench-detect` vs the baseline table | equal numbers |
| P7 node0 | `ct-bench-detect from-export` on the two saved bench runs, then `a2a-bench score` | 1.000/1.000 and 1.000/0.883, 89.1 FP/1k |

P2 and P3 can run in parallel with P4. Only after P6 and P7 pass is
ct-eval's scoring removed from crosstalk.

### 7.3 Expected differences, decided up front

- **Report shape.** `report.json` changes schema (bench names); P2
  compares counts, not bytes.
- **`DetectionQuality` cross-check.** Moves to the adapter's tests.
- **AI Village `@2`.** Different labels by design (§5.2); reported as a
  separate row after parity, not part of it.

## 8. Versioning

- **Format version** `a2a-bench/<major>`: in every header and the
  manifest. A reader accepts exactly its major. Any change to part text,
  canonical JSON, id derivation, alignment or a row's fields is a new
  major. No compatibility shims (none are asked for); old exports are
  re-exported.
- **Dataset version** `<dataset>@<n>`: bumped whenever a converter
  change alters any exported byte for the same source revision and
  selection (label fixes, new controls, pace changes). The bump is
  recorded in `docs/datasets/<dataset>.md` with what changed and why, and
  P4-style golden diffs make an unintended bump fail CI on fixtures.
- **Source revision**: the manifest records the dataset's own revision
  (HF snapshot hash from the symlink target, git HEAD for clones) and a
  BLAKE3 digest over the files read. `a2a-bench export` refuses to run
  against a source whose revision differs from the pinned one in
  `datasets.toml` unless `--allow-revision` is given, and then records the
  new one.
- **Scores** cite `(format, dataset@n, source revision, selection, detector
  name/version/variant)`. Gates are keyed by dataset id and version, so a
  dataset bump makes its old gates skip with a reason instead of passing
  silently.
- **No dataset bytes are committed.** Fixtures are synthetic and
  generated by tests, as now.

## 9. Holdout split

Approved: part of the data is held out, and crosstalk's developers score
against it only through release runs.

### 9.1 What can be held out

A holdout is only worth something if the detector was never tuned on it.
crosstalk-rollouts' account (2026-10-05) of what has been seen
distinguishes **scored** (ct-eval run, reference or live) from
**profiled** (read for statistics that shaped converters and, in places,
detector findings sent to crosstalk-impl). Profiled data counts as seen:
those statistics (boilerplate shingle rates, AgentDojo's escape
distribution) reached the detector's design. So the holdout is drawn only
from data neither scored nor profiled, and everything else is **dev**,
including every parity selection (§7.0).

| Dataset | Seen (dev) | Holdout candidate |
| --- | --- | --- |
| SALT (2,650 traces = 53 conditions × 50 reps) | reps 1–5 of each condition (`--limit 53` and `--limit 265`), an early 50-file sample, a handful of profiled reps | a stratified share of reps 6–50 not in the early sample or the profiled list (the exact files are recorded before the split is cut) |
| AI Village (2025-04 → 2026-09) | the Claude Code stream (993 contexts, 2026-01-26..03-31), window week 2026-07-13..17, profiled week 2026-02-02..06; whole-dataset aggregate counts | window-mode weeks outside those; only aggregate counts were ever taken over them. Weaker than SALT's: tie-breaking rules were designed with whole-dataset pair counts in view |
| open-swe | train-00000 of the 13 downloaded folders | the ~202 shards never downloaded |
| lmcache (767 sessions) | `--count 16`, ~200 profiled sessions | the remaining unseen sessions, once the profiled ones are identified |
| swe-splice, cipher | splice seed 0 `--count 80`, cipher `--count 50` | fresh generation seeds |
| demo-swarm | bench runs 20261005T155320Z, 184212Z, 184633Z, 20261006T020835Z, 021639Z | fresh seeds and runs, if the generator is not tuned on them |
| collusion-wiki, AgentDojo, τ²-bench, swarm-traces | fully scored or fully profiled | none |
| Who&When, MAST, Moltbook, trace-commons | never converted (profiled only) | clean once converters exist; later work |

crosstalk-impl's own ct-eval runs are confirmed for SALT `--limit 53`,
wiki, swarm, AgentDojo and node0 replays; anything beyond is asked of it
before a split is cut. The unit is a world (worlds are independent),
stratified by the dataset's own conditions so the holdout measures the
same mix as dev.

### 9.2 How it is kept out of dev runs

- **Splits are dataset versions.** `<dataset>@n` exports carry
  `split: dev | holdout` in the manifest. The dev split is an explicit,
  committed list (`splits/<dataset>@<n>.dev.toml`: world keys and the
  source revision); the holdout is its complement in the pinned source
  revision and selection. A dev list never changes within a version.
- **Dev is the default and the only thing the normal commands export.**
  `a2a-bench export` exports dev worlds only. `--split holdout` also
  needs `--release <detector>@<tag>`, writes outside every repository
  (`~/.local/share/a2a-bench/releases/<detector>/<tag>/`), and refuses a
  detector that is not at a tagged commit (its header's `version` must
  name the tag).
- **Holdout reports are aggregate.** For a holdout run the scorer writes
  the overall, per-tier and per-route rows and the gate outcomes, and no
  per-label, per-world or example output (no missed-label lists, no
  violation sources, no diagnostics with excerpts), so a release report
  cannot be mined for the labels.
- **Commitment.** Each holdout's world list and labels digest is
  committed as a BLAKE3 hash (`splits/<dataset>@<n>.holdout.commit`), so
  any later release can be checked against the same holdout without
  publishing it.

This is procedural protection, not secrecy: the data sits on the same
machine, so anyone can run a converter over it. The protection is that
no normal command produces holdout files, and that the rule is written
down for every session.

### 9.3 Who may run it

- The bench owner (this session, or its successor in the bench repo),
  or a release job, when the user or crosstalk-integration asks for a
  release score of a tagged detector.
- Not crosstalk's development sessions (impl, rollouts, ui, …). They get
  the aggregate report only. The bench's `CLAUDE.md` and README say so,
  and crosstalk-integration relays it.
- A holdout run is never used for tuning. If a release run shows a
  regression, the fix is diagnosed on dev data.

## 10. Work plan after this design is approved

Workstreams, each a branch in its own worktree
(`~/Code/ai/a2a-transmission-bench/<branch>`):

1. `feat/format` — `a2a-bench-format`: types, checked constructors,
   JSONL IO, canonical JSON, part text, ids; tests first (round trips,
   rejections, canonical JSON vectors). Everything else depends on it.
2. `feat/score` — scorer, report, gates (after 1).
3. `feat/reference` — `a2a-reference` (after 1).
4. `feat/datasets-*` — converters, several in parallel (after 1):
   `salt`, `agentdojo-tau2`, `wiki-swarm`, `swe-synthetic` (splice,
   cipher, open-swe, lmcache), `ai-village` (with `a2a-bench-resource`),
   `demo-swarm`.
5. crosstalk side (crosstalk-rollouts and crosstalk-integration):
   `feat/eval-bench-export` (7.1) now; `feat/bench-adapter` after 1.
6. `test/parity` — the P1–P7 harness and baseline.

Expected sibling conflicts: the converter branches all touch
`a2a-bench-datasets/src/lib.rs` and `datasets.toml`; the CLI branch
touches every crate's registration. Acceptable; resolved at merge.

## 11. Decisions (user, 2026-10-05)

1. **Holdout:** yes; designed in §9.
2. **Repository:** private, `RhizoNymph/a2a-transmission-bench`. Public
   later, once parity is proven and the holdout is in place.
3. **Dependency:** crosstalk depends on `a2a-bench-format` through a git
   tag, with the exact revision pinned by `Cargo.lock`. No mirrored
   structs.
4. **Baseline:** the crosstalk staging commit that includes #105; every
   baseline is re-measured there, and the brief's SALT figures are not
   trusted.
