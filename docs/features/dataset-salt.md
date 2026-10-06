# SALT converter (`a2a-bench-dataset-salt`)

SALT-NLP's "Emergent Collusion in Long-Horizon LLM Agent Interaction":
two agents (Alice and Bob) per trajectory, exchanging messages through a
harness-mediated channel over several episodes. 2,650 trace files,
53 conditions × 50 repetitions. The crate (`crates/datasets/salt`) turns
each trace file into one checked world. Dataset id `salt`, dataset version
1, which reproduces crosstalk-eval's corpus at 7f8a2fb (`datasets/salt`).

## Scope

- Discovering trace files (`traces/<experiment>/<condition>/repNNN.json[.gz]`)
  in stratified order, with `limit` and `include` (crosstalk-eval's
  `--limit`, `--include`), recorded in the manifest's `selection` as
  `limit` (int) and one `include` key holding a list of texts, in the
  order given.
- Reading plain and gzipped traces; converting OpenAI-chat messages to
  bench messages.
- Reconstructing each agent's calls per episode, timing them on the
  virtual clock (`Pace`), and building exchanges.
- Labels: delivered messages (construction or forwarding tier), rejected
  sends, scripted-peer sends, shared sources, boilerplate.
- `SaltSource`, a `TraceSource` that reads one file per world, skips files
  the export will not keep, and records the files it read for the source
  digest.

## Non-scope

- The CLI, the export itself (`a2a-bench-corpus::export`), revision
  pinning: other crates.
- Scoring forwarding apart from `overall`, gates, detectors: the scorer
  and detectors. This crate only assigns the `forwarding` tier.
- The reference matcher: the fold and shingle hash the forwarding tier
  needs are ported here, not shared with it.
- Dataset bytes: the fixtures under `tests/fixtures/salt/` are synthetic
  (crosstalk-eval's, unchanged).

## Data and control flow

```text
source(root, &Options { limit, include }, pace)
  files::discover ── traces/<exp>/<cond>/ sorted, files filtered by include,
                     interleaved round-robin across conditions, truncated to limit
  ──▶ SaltSource { files, pace, filter: All, read: FilesRead }

TraceSource::select(filter)   remembers the filter (world key = file path
                              without "traces/" and extension)
TraceSource::worlds()         per file: skipped unless filter keeps its key;
                              else read.record(file), load_world_paced

load_world_paced(root, relative, pace)
  read (gunzip when .gz) ──▶ serde_json ──▶ schema::Trace
  ──▶ convert_trace(trace, "/"-separated relative path, pace)

convert_trace
  declare: every agent named by any episode, in name order;
           model-driven iff any accepted llm_usage entry, model = run config's route
           (or "unknown"); scripted otherwise, with that model
           (WorldBuilder::scripted_agent_with_model)
  episode_start = 0
  for each episode (position p):
    for each agent: episode::reconstruct(agent, episode, file, scripted, {pace, episode_start})
      messages::convert each raw message (Message::new)
      region start = last user turn whose first line is "## Episode N: task phase"
      responses   = assistant messages from the region start
      fidelity    = reconstructed if scripted or #responses == #accepted calls, else synthetic (warn)
      call_events = region tool calls (message, call) zipped in order with the agent's sorted event ids
      delivered   = region user turns "[round=…][from=x][type=…]\n\n<content>" matched to an
                    unused transcript entry (receiver, sender, content equal) → its event id
      clock::clock: one Turn per response, timed (see "The virtual clock")
    for each model-driven agent, each turn:
      ExchangeDraft { request = messages[..index], response = [messages[index]],
                      model = usage.requested_model or the declared model,
                      stop = finish_reason mapped, tools = None, fidelity,
                      source = (file, /results/p/agents/<name>/messages/<index>) }
      ──▶ builder.exchange (id derived from dataset, source, time)
      ──▶ Carriers.add (first exchange per (message, agent) and per message)
    episode_start += episode_steps(episode)
    truth::EpisodeLabels::label ──▶ Drafts: deliveries, rejected sends, shared + boilerplate
  place(drafts, carriers): ids t0, t1, … in truth order; places without an exchange
                           go in the first carrier ──▶ builder.label
  builder.finish(Complete { construction })
```

### Messages (`messages.rs`)

| SALT | Bench |
| --- | --- |
| `system` | `System([Text])` |
| `user` | `User([Text])` |
| `assistant` | reasoning parts, the text (when not empty), tool calls |
| `tool` | `Tool([ToolResult { call_id: tool_call_id or "", [Text], outcome }])`, outcome `error` when the text is a JSON object with `"success": false`, else `success` |

`content` is a string, `null` (empty text) or a list whose parts' `text`
members are joined with `\n`. Assistant reasoning, in order: each
`thinking_blocks` entry with thinking text is `reasoning` with that text
(its signature dropped; signed visible thinking is not opaque), one
without text but with `data` or a non-empty signature is
`reasoning_opaque`; without thinking blocks, `reasoning_content` (else
`reasoning`) when a non-empty string is `reasoning`. Then one
`reasoning_opaque` per `reasoning_items[].encrypted_content` and per
string in `provider_specific_fields.thought_signatures`. Tool calls keep
their ids whole as `call_id`, Gemini's `__thought__` + base64 signature
included: an id is never part text. Arguments: a string is canonical JSON
when it parses, else `invalid` verbatim; `null` is `invalid ""`; an object
is canonicalised from its serialisation.

### Labels (`truth/`)

Per episode, in this order (ids count the world's truth: `t<index>`):

1. **Deliveries.** Each `channel_transcript` entry whose sender and
   receiver are agents of the trace, whose receiver is model-driven, whose
   delivered turn was matched and is followed by a receiver call:
   - sender scripted (`controlled_peer` Bob): a `no_sender_exchange`
     control (construction) at the content, reader exchange set, with the
     content as text;
   - else a `transmission`: Direct route, `user_turn` carrier, at the
     content bytes (after the header) of part 0 in the reader's first call
     after the turn, `sender_exchange` the exchange whose response made the
     send (when the event matched a call), needs
     the format's `MatchNeed::through_json_string(content)`, tier `forwarding` when the
     sender relayed its own tool output (below), else `construction`.
2. **Rejected sends.** Each `send_message` event with `success: false`
   between two model-driven agents, matched to its call: a `rejected_send`
   control (construction), origin = the call's whole argument text, text =
   the arguments' `content` (else the event's cut content), no reader
   exchange.
3. **Shared sources and boilerplate** (structural), for each model-driven
   reader and each other agent as `from`, over every message of the
   reader's list with non-blank text:
   - message 0 when a system message: `shared_source`, once per reader per
     world (keyed by message id), no reader exchange;
   - region tool results answering a call to `inspect_database`,
     `query_database`, `read_code`, `read_source` or `resolve_records`:
     `shared_source`, reader exchange = the next call (may be none);
   - region user turns that are not matched deliveries: `boilerplate`,
     reader exchange = the next call (skipped when none).
   Each is located over the whole of part 0.

**Placing controls without an exchange.** A bench location names its
exchange; crosstalk-eval's did not. A control's `at` without a reader
exchange (system prompts, trailing shared results) and every origin go
in the first exchange, in world order (time, agent, id), of the reader
(for an origin, the sender) whose request or response carries the
message, else the world's first exchange carrying it; the control's
`reader_exchange` stays absent. This is crosstalk's golden export's rule
(`golden/labels.rs`). A message no exchange carries fails the world
(`SaltError::UncarriedLocation`), as it fails the golden export.

### Forwarding (`forwarding/`)

A delivery is `forwarding` when at least half (`FORWARDED_SHARE` = 1/2)
of its content's folded bytes is covered by `FORWARD_K` = 24-byte
shingles found in a tool result of the sender's list before the message
that made the send. Folding (`forwarding/fold.rs`) undoes string escapes
at any depth (`\n`, `\t`, `\r`, `\b`, `\f` become a space, `\uXXXX` its
character with surrogate pairs, an escaped line break and the next line's
indentation vanish, any other escaped character stays), lowercases, and
collapses whitespace runs into one space (leading whitespace dropped).
Shingles (`forwarding/shingle.rs`) are a polynomial rolling hash (base
`0x100000001b3`, byte + 1) over every 24-byte window. Content under 24
folded bytes never forwards. Both are crosstalk-eval's reference matcher
code, ported bit for bit.

### The virtual clock (`episode/clock.rs`)

Nothing in SALT has a time. Each episode-global event id is a step of the
pace (`Pace::at(step, 0, sub)`, 1 to 5 s per step by default), episodes
back to back: episode `k` starts at the sum of the earlier episodes'
`episode_steps` (max event or delivery id + 3; 2 when there are none).
Walking an agent's region, `floor` rises to event + 1 at each delivered
turn and each tool result of a matched call. A response whose first tool
call has event `e` with `e + 1 ≥ floor` is step `start + e + 1`, sub 0;
any other is step `start + floor`, sub 1; then `floor` rises to that step
offset. A time not after the agent's previous one becomes the previous
time + 1 µs. So a sender's calling exchange precedes the reader's call
that first carries the delivery. Exchange ids carry these times.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `Cargo.toml` | package `a2a-bench-dataset-salt` | |
| `src/lib.rs` | crate root, options, the source, file reading | `DATASET`, `VERSION`, `Options` (`settings`), `source`, `SaltSource` (`files`, `files_read`, `pace`, `TraceSource`), `load_world`, `load_world_paced`, `convert_trace`, `SaltError` |
| `src/error.rs` | the error enum | `SaltError` |
| `src/files.rs` | discovery, stratified order, world keys | `discover`, `world_name`, `source_file` |
| `src/schema.rs` | the trace fields read | `Trace`, `RunConfig`, `Episode`, `AgentRecord`, `RawMessage`, `RawToolCall`, `RawFunction`, `Delivery`, `Event`, `Usage` |
| `src/messages.rs` | SALT messages to bench messages | `content_text`, `body`, `convert`, `arguments`, `argument` |
| `src/episode/mod.rs` | per-agent episode reconstruction | `reconstruct`, `AgentEpisode`, `Turn`, `DeliveredTurn`, `delivered_turn`, `is_task_marker`, `stop_reason` |
| `src/episode/clock.rs` | call times | `episode_steps`, `EpisodeClock`, `ClockInputs`, `clock` |
| `src/world.rs` | one trace to one world | `convert_trace` |
| `src/truth/mod.rs` | the episode's labels as drafts | `EpisodeLabels`, `Labelled`, `SHARED_TOOLS` |
| `src/truth/place.rs` | drafts, the carrier index, ids and placement | `Draft`, `Delivery`, `Control`, `Spot`, `Carriers`, `place` |
| `src/forwarding/mod.rs` | the forwarding rule | `ToolOutput`, `FORWARD_K`, `FORWARDED_SHARE` |
| `src/forwarding/fold.rs` | the matching fold (text only) | `fold` |
| `src/forwarding/shingle.rs` | rolling-hash shingles, covered runs | `shingles`, `covered` |
| `tests/salt.rs` | crosstalk-eval's SALT tests on bench types, plus options, stratification, placement, ids, the source | |
| `tests/forwarding.rs` | the forwarding tier on the fixture and synthetic traces; the rule's unit tests | |
| `tests/clock.rs` | episode steps and call times | |
| `tests/real_data.rs` | ignored: summarises a real selection (ids, counts, ranges; no text) for the comparison below | |
| `tests/common/mod.rs` | fixture paths, label accessors, a trace builder | |
| `tests/fixtures/salt/traces/…` | three synthetic traces (main, memory_scope, controlled_peer), crosstalk-eval's | |

## Invariants and constraints

- **Parity.** Same worlds in the same order, exchanges with the same
  source references and times (so the same ids), messages part for part,
  labels in truth order with crosstalk-eval's fields. Checked on real data
  below; byte-level against the golden export at P4.
- **One world per file**, key = relative path without `traces/` and
  `.json[.gz]`. Times are deterministic in the pace; nothing reads the
  wall clock or a random source.
- **Streaming.** One trace in memory at a time. Files skipped by
  `select` are never opened; `files_read` lists exactly the files read.
- **Text.** Locations index bench part text; label text equals the text
  at its location (checked by `World::new`). No signature or encrypted
  payload is stored.
- **Coverage** is `complete` at `construction`: the logged channel is the
  only way the agents talk.
- No unwrap/expect/panic outside tests; errors are `SaltError`.

## Known crosstalk-eval quirks kept

- `world_name` strips `.gz` and `.json` with `trim_end_matches`, so
  repeated suffixes all go.
- The region start is the *last* task-phase marker; a list with none
  starts at 0, so all its assistant messages are calls of the episode.
- A carried-over list whose episode added no task marker re-counts the
  previous episode's calls as this episode's.
- Tool calls are matched to events by order only, even when the counts
  differ (logged at debug); a response's time uses its *first* call's
  event.
- Accepted `llm_usage` entries are paired with turns in order only for
  `requested_model` and `finish_reason`; a mismatch only makes the
  episode's exchanges `synthetic`.
- A system prompt is labelled once per reader per world, for the first
  peer only (with more than two agents the others get none).
- Shared-source controls on a tool result after the reader's last call
  keep no reader exchange; boilerplate there is dropped.
- An empty-content delivery (empty range) or a rejected send with empty
  arguments is silently unlabelled.
- Scripted agents are declared with the run config's model, as
  crosstalk-eval declares every agent with one
  (`WorldBuilder::scripted_agent_with_model`).

## Differences from crosstalk-eval

None in content. Representation only:

- Locations name an exchange (placement rule above); crosstalk-eval's
  named a message only.
- Label ids `t<index>` are new (the golden export's).
- Message ids hash bench bodies, which drop signatures; two spec messages
  that differ only in a signature are one bench message. The carrier index
  is keyed by bench message id where the golden export keys by spec hash;
  the two agree unless a reader's history holds two such messages, which
  no SALT trace does (system prompts carry no signatures).
- Token usage (`TokenUsage`) is not part of the format and is dropped.
- The `include` selection is one list-valued key; crosstalk's golden
  export writes `include[0]`, `include[1]`, ….

## Real-data comparison

Both sides on the same selections of `~/Data/ai/agents/salt-nlp`
(default pace, seed 0): ct-eval 7f8a2fb release (`truth`, `run --detector
reference`, `run --detector pipeline` for per-world exchange counts) and
this crate (`tests/real_data.rs`). Labels were compared row by row per
world in truth order: kind, sender, reader, reader and sender exchange
ids, part and byte range of `at` and `origin`, tier, route, carrier,
needs, reason, whether text is set, and source reference.

| | `--limit 53` (one per condition) | | `--limit 8` | |
| --- | --- | --- | --- | --- |
| | ct-eval | bench | ct-eval | bench |
| worlds (keys and order equal) | 53 | 53 | 8 | 8 |
| world failures | 0 | 0 | 0 | 0 |
| agents | 106 | 106 | 16 | 16 |
| exchanges (equal in every world) | 11,796 | 11,796 | 1,184 | 1,184 |
| exchange ids named by ct-eval labels, all present in bench | 9,907 | 9,907 | 953 | 953 |
| transmissions | 3,850 | 3,850 | 200 | 200 |
| … tier construction / forwarding | 2,909 / 941 | 2,909 / 941 | 160 / 40 | 160 / 40 |
| … route direct, carrier user_turn | 3,850 | 3,850 | 200 | 200 |
| … needs exact / decoded json_string | 2,806 / 1,044 | 2,806 / 1,044 | 121 / 79 | 121 / 79 |
| negative controls | 12,951 | 12,951 | 1,443 | 1,443 |
| … boilerplate (structural) | 9,250 | 9,250 | 960 | 960 |
| … shared_source (structural) | 1,959 | 1,959 | 183 | 183 |
| … rejected_send (construction) | 992 | 992 | 0 | 0 |
| … no_sender_exchange (construction) | 750 | 750 | 300 | 300 |
| worlds whose label rows differ | | 0 | | 0 |
| fidelity | | all reconstructed | | all reconstructed |

No difference. Exchange ids not named by any label (1,889 and 231) were
not compared directly; they derive from the same source references and
times as the compared ones, and per-world counts are equal. The bench run
of `--limit 53` takes about 30 s in release (361 MB peak).
