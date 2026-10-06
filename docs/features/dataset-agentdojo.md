# AgentDojo converter (`a2a-bench-dataset-agentdojo`)

The AgentDojo converter (`crates/datasets/agentdojo`): prompt-injection
runs of tool-using agents as worlds of a victim and a synthetic attacker.
It is crosstalk-eval's converter at commit `7f8a2fb`
(`src/datasets/agentdojo/`) ported onto the bench's format and corpus
types. **`agentdojo@1` reproduces ct-eval's corpus exactly**, with the
differences listed under [Differences from ct-eval](#differences-from-ct-eval).
It depends on `a2a-bench-format` and `a2a-bench-corpus` only.

## Scope

- Run discovery under `runs/<pipeline>/<suite>/<task>/<attack>/<file>.json`,
  the `include` filters and the stratified order, `limit`.
- One world per run: the victim's exchanges, the synthetic attacker's one
  exchange, paced by the corpus `Pace` clock.
- Labels: injection copies (construction or out of reach), boilerplate
  controls; coverage `complete {construction}`.
- The arrival tally (slot and label classes, second hop) for reports.
- Recording the files read (`FilesRead`) for the source digest.

## Non-scope

- The CLI (`a2a-bench export`), `datasets.toml` and revision pinning.
- Resource canonicalisation: route resources are written as ct-eval
  wrote them; the scorer canonicalises (`a2a-bench-resource`).
- The reference matcher and scoring (ct-eval's
  `the_reference_finds_injections_it_can_route` test belongs to the
  reference and score stages, P3/P2).
- Labelling the second hop (victim → attacker): it is counted only.
- Dataset bytes: tests use synthetic fixtures.

## Data and control flow

```text
source(root, &Options, Pace)
  files::discover(root, options)
    runs/ missing ──▶ NoRuns
    walk pipeline/suite/task/attack dirs (each sorted, filtered by key=value)
    *.json files, every substring filter holding
    group by (pipeline, suite, attack), groups sorted, runs sorted by (task, path)
    interleave groups round-robin, truncate to limit
  ──▶ AgentDojoSource {files, filter: All, tally, files_read, pace}

TraceSource::select(filter)   world keys are run paths: dropped runs are never read
TraceSource::worlds()         for each kept run, in order:
  files_read.record(relative)
  load_world(root, relative, pace)
    fs::read ─▶ serde_json ─▶ schema::Run
    convert_run(run, file, pace)
      WorldBuilder(dataset "agentdojo", key = path under runs/ without .json)
      victim  = model_agent("victim", model_of(pipeline))   (defense suffix dropped)
      messages::convert(run.messages) ─▶ Conversation {messages, calls}
      attacked (≥1 injection)?
        attacker = model_agent("attacker", "agentdojo-attacker")
        exchange at pace.at(0,0,0): request = one user message naming the attack,
          response = one assistant message, a text part per non-empty injection,
          model "agentdojo-attack/<attack>", Synthetic, source /injections
      per assistant message i: exchange at pace.at(i+1,0,0),
          request = messages[..i], response = [messages[i]],
          stop tool_use|end_turn, Reconstructed, source /messages/<i>
      truth::RunLabels::label(&mut tally)
        no attacker ──▶ no labels
        injections: per tool message with a later victim exchange,
          per distinct injection text, per occurrence (classify::Output):
            transmission attacker→victim, reader = first victim exchange after it,
            route = route::expected_route(answered call), carrier tool_result,
            needs = arrival need (Unobserved{sender medium unobserved} on a channel),
            tier construction | out_of_reach, source /messages/<i>/injections/<vector>/<copy>
        boilerplate: per system/user message with text: control attacker→victim,
            no reader exchange, at = whole part in the victim's first exchange
            carrying it (left out when none does), source /messages/<i>
        indicators::second_hop (tally only)
      builder.label(…) per label, finish(Complete{Construction})
  tally.add(run tally)
```

### Messages (`messages.rs`)

| AgentDojo | Bench |
| --- | --- |
| `system` | `System([Text])` |
| `user` | `User([Text])` |
| `assistant` | `Assistant`: the text (when not empty), then the tool calls |
| `tool` | `Tool([ToolResult])`: the error text and `error` when `error` is non-empty, else the content and `success` |

Content is a string or blocks (`content` or `text` of each, joined by
`\n`). Calls without ids get `agentdojo-call-<message>-<position>`; a result
answers the pending call with its `tool_call_id`, else the earliest pending
call equal to its `tool_call` (function and args), else the earliest
pending call, else gets `tool_call_id` or `agentdojo-result-<message>`.
Arguments are `serde_json::Value::to_string` (`null` → `{}`), canonical
JSON when it parses.

### Arrival classes (`classify.rs`)

Each injection, trimmed, is searched in the raw output, then in the
whitespace-folded output, the JSON-unescaped and the YAML-unescaped output
(each folded); every non-overlapping copy keeps the weakest class that
finds it, mapped back to raw bytes. `Exact` → `exact`, `Whitespace` →
`normalized`, `JsonString` → `decoded [json_string]`, `YamlString` →
`decoded [yaml_string]`.

### Routes (`route.rs`)

`get_webpage(url)` → `channel {url}` with ct-eval's own URL text
(`parse_url`, `locator_key`: scheme and host lower case, default port and
fragment dropped, empty path `/`, query parameters sorted; a scheme-less URL
read as `http://`). `read_file(file_path)` → `channel {file}` rooted at
`/`, `.`/`..` resolved. Anything else, or an unparsable URL, → `direct`.

### Ids

Exchange ids are derived by the corpus builder from the dataset id, the
source reference and the paced time, as ct-eval derives them. Labels are
`t<n>`, `n` being the label's index in ct-eval's truth for the world
(positives in message order, then controls), which is the golden export's
numbering.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/datasets/agentdojo/src/lib.rs` | crate root, constants | `DATASET`, `VERSION`, `VICTIM`, `ATTACKER` |
| `src/options.rs` | selection flags and their manifest record | `Options` (`limit`, `include`, `settings`) |
| `src/source.rs` | the dataset as a `TraceSource` | `source`, `AgentDojoSource` (`files`, `tally`, `files_read`, `pace`) |
| `src/convert.rs` | one run to one world | `load_world`, `convert_run`, `model_of`, `Loaded` |
| `src/error.rs` | the converter's errors | `AgentDojoError` |
| `src/files.rs` | run discovery, filters, stratified order | `discover`, `RunFile`, `world_name` |
| `src/schema.rs` | the run JSON | `Run`, `RawMessage`, `Content`, `Block`, `RawCall` |
| `src/messages.rs` | messages to bench messages, call pairing | `convert`, `Conversation` |
| `src/classify.rs` | how an injection arrived | `Arrival`, `Occurrence`, `Output`, `occurrences` |
| `src/route.rs` | expected route of a read | `expected_route`, `parse_url`, `normalize_path` |
| `src/location.rs` | locations, first carrying exchange | (crate-private) |
| `src/truth/mod.rs` | labels | `RunLabels`, `Attacker`, `SENDER_MEDIUM_UNOBSERVED` |
| `src/truth/indicators.rs` | the second hop | `second_hop`, `indicators` |
| `src/tally.rs` | arrival and second-hop counts | `Tally`, `ArrivalCounts`, `SecondHop` |
| `tests/agentdojo.rs` | ct-eval's tests on bench types, plus pacing, ids, numbering, selection, export | |
| `tests/parity.rs` | every fixture label equals ct-eval's, field by field | |
| `tests/fixtures/runs/` | ct-eval's synthetic runs (generated, not dataset bytes) | |
| `tests/fixtures/crosstalk-7f8a2fb-truth.jsonl` | `ct-eval truth --dataset agentdojo --root tests/fixtures` at 7f8a2fb | |

## Invariants and constraints

- **Parity.** Same worlds in the same order, same exchanges (ids, times,
  messages part for part), same labels as ct-eval 7f8a2fb, except the
  differences below. Checked on the fixtures (`tests/parity.rs`) and on
  real data (below).
- **Determinism.** Discovery sorts every directory; the pace is the only
  source of times; label order is ct-eval's truth order.
- **Coverage** is `complete {construction}` for every run, attacked or not.
- **Times.** The attacker is call step 0, the victim's response to message
  `i` is step `i + 1`; `minor` and `sub` are 0.
- **Streaming.** One run is read and converted at a time; the filter
  given to `select` skips runs before they are read.
- **Reads only.** The converter reads run files and never acts on their
  content.

## Known ct-eval quirks kept

- The attacker's declared model is `agentdojo-attacker` (so its vendor is
  `agentdojo-attacker`) while its exchange names `agentdojo-attack/<attack>`.
- Empty injections are left out of the attacker's response but still count
  as slots (`absent`).
- Every message takes a pace step (the response to message `i` is step
  `i + 1`), so tool runs and prompts between two calls take time too.
- An assistant message with no text and no calls is an empty assistant
  message, and still an exchange.
- A failed call's result shows only the error text; the content is dropped.
- An unpaired result falls back to the earliest pending call even when its
  recorded `tool_call` differs.
- Copies in a tool output after the victim's last call get no label, and
  their vector stays `absent` unless another copy was read.
- Channel URLs keep ct-eval's naive normal form, not the bench's canonical
  one (the scorer canonicalises).
- Runs without an attack keep `complete {construction}` coverage with no
  labels; `injection_task_*/none` runs have no attacker though their user
  prompt is the attacker's goal.
- The second hop is counted, never labelled.

## Differences from ct-eval

| Difference | Why |
| --- | --- |
| A boilerplate control on a prompt no victim exchange carries is left out (its `t<n>` stays unused). Real data: 44 controls in 22 runs that have no assistant message at all; none in the documented selection. | A bench location names an exchange that carries its message; ct-eval's control names only the message. crosstalk's golden export refuses these worlds (`Gap::UncarriedLocation`); a control no exchange carries guards no prediction, so leaving it out changes no score. |
| A control with no reader exchange is placed in the victim's first exchange carrying the prompt. | ct-eval's control names a message, not an exchange; this is the golden export's placement. Its `reader_exchange` stays absent, so it still covers every exchange. |
| Labels have ids `t<n>`. | The format needs ids; this is the golden export's numbering. |
| No token usage, protocol or transport on exchanges. | The format has no such fields. |
| Message ids are the bench's. | By design: the bench hashes its own canonical form. |
| The source digest covers the run files read. | The brief's `FilesRead`; crosstalk's golden export digests every file under the dataset directory instead, so the manifests' `source.digest` differ. |

## Real-data comparison (ct-eval 7f8a2fb release vs this port)

Dataset `~/Data/ai/agents/agentdojo` at `089ed468`. Counts only. Labels are
compared field for field by id (`t<n>`); exchange ids are compared on the
set every label names (ct-eval's outputs name no others).

Documented selection (in full): `--include pipeline=gpt-4o-2024-05-13
--include pipeline=claude-3-5-sonnet-20241022 --include
pipeline=gemini-1.5-pro-002 --include attack=important_instructions
--include attack=none`.

| | ct-eval | port |
| --- | ---: | ---: |
| worlds (every labelled world's key, in the same order) | 2,259 | 2,259 |
| agents | 4,146 | 4,146 |
| exchanges | 11,235 | 11,235 |
| exchange ids named by labels (identical sets) | 3,606 | 3,606 |
| transmissions, direct, tool_result, construction | 2,324 | 2,324 |
| — of which `normalized` / `decoded` | 294 / 2,030 | 294 / 2,030 |
| transmissions, channel url, out_of_reach | 250 | 250 |
| transmissions, channel file, out_of_reach | 109 | 109 |
| negative controls, boilerplate, structural | 3,774 | 3,774 |
| labels differing in any field | | 0 |
| arrival slots exact / whitespace / json / yaml / absent | 0 / 645 / 986 / 1,055 / 275 | same |
| second hop | 339 of 414 | same |

Whole dataset (36,679 runs): transmissions 51,220 = 51,220 (construction
45,259, out of reach 5,961; direct 45,259, url 4,024, file 1,937; exact
3,296, normalized 12,262, decoded 29,701, unobserved 5,961), exchange ids
named by labels 64,791 = 64,791, no field differences, no failures; negative
controls 93,778 vs 93,734: the 44 uncarried controls above.
