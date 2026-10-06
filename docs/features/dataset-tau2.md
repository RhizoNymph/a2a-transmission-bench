# τ²-bench converter (`a2a-bench-dataset-tau2`)

The τ²-bench converter (`crates/datasets/tau2`): customer-service
simulations as worlds of two model agents, the agent and an LLM user
simulator. It is crosstalk-eval's converter at commit `7f8a2fb`
(`src/datasets/tau2/`) ported onto the bench's format and corpus types.
**`tau2@1` reproduces ct-eval's corpus exactly**, with the differences
listed under [Differences from ct-eval](#differences-from-ct-eval). It
depends on `a2a-bench-format` and `a2a-bench-corpus` only. This is a benign
baseline for precision: there are no attacks.

## Scope

- Results files (`*.json` directly under the dataset root,
  `tau2-bench/data/tau2/results/final`), `include` substrings of file
  names, `limit` spread over files.
- One world per simulation: each side's view of the conversation, one
  exchange per model call at its recorded time, reconstructed system
  prompts.
- Labels: text turns (structural transmissions), boilerplate and
  shared-source controls; coverage `complete {structural}`.
- Recording the files read (`FilesRead`) for the source digest.

## Non-scope

- The CLI, `datasets.toml`, revision pinning.
- The pace clock: τ² records its own times (the manifest's `pace` is empty).
- Token usage (the format has no field for it).
- The reference matcher and scoring (ct-eval's
  `the_reference_finds_every_turn_with_no_false_positive` test belongs to
  the reference and score stages).
- Dataset bytes: tests use synthetic fixtures.

## Data and control flow

```text
source(root, &Options)
  files::discover: root not a dir ──▶ NoResults; *.json files directly under it,
                   every include substring in the name, sorted
  ──▶ Tau2Source {files, limit, filter: All, files_read}

TraceSource::worlds()   one results file parsed at a time
  quota = ceil(limit / files)      (none without a limit)
  for each file: files_read.record; load_results ─▶ schema::Results
                 (no limit and a filter keeping nothing of this file: not read)
    picks = files::pick(simulations, quota)   evenly spaced, i·n/quota
    for each pick, until `limit` picks overall (a failed file counts one):
      filter drops its key ─▶ skipped (still counted)
      convert_simulation(results, file, index)
        task = the simulation's task (UnknownTask if none)
        WorldBuilder(dataset "tau2", key "<file without .json>/<index>")
        agent = model_agent("agent", agent llm | "unknown")
        user  = model_agent("user", user llm | "unknown")
                when implementation != dummy_user or any record is a user turn
        prompts: agent_system_prompt (fidelity), user_system_prompt
        per side: views::view(side, records, prompt)
          per entry that is the side's own model call (raw_data not null):
            at = time::parse_time(timestamp) (MissingTime without one),
                 bumped to previous + 1 µs when not later
            exchange: request = [system] + entries before it, response = [entry],
              stop from finish_reason, the side's fidelity,
              source /simulations/<i>/messages/<raw>
        truth::SimulationLabels::label (records in order)
          assistant/user text turn with a peer and a later peer exchange:
            no raw_data or relays its sender's prompt ─▶ boilerplate control
              (reader exchange, at = whole text, text)
            else transmission sender→peer, direct, user_turn, exact, structural,
              sender exchange = the turn's own, at = whole text in the reader exchange
          tool result (requestor user|assistant) with text, both sides present:
            shared_source control peer→reader, reader exchange = next one (or none),
            at = whole text in the reader exchange, else in the reader's first
            exchange carrying it, else left out
        finish(Complete{Structural})
```

### Views (`views.rs`)

| record | agent sees | user simulator sees |
| --- | --- | --- |
| `assistant` | `Assistant` (text, tool calls) | `User` (its text only; nothing for a bare tool call) |
| `user` | `User` (its text only; nothing for a bare tool call) | `Assistant` (text, tool calls) |
| `tool`, requestor `assistant` (default) | `Tool` | nothing |
| `tool`, requestor `user` | nothing | `Tool` |

A tool result's call id is its `id` (else `tau2-result-<raw>`), its outcome
`error` when `error` is true. Call arguments are `Value::to_string`,
canonical JSON when they parse.

### Prompts (`prompts.rs`)

Rebuilt from τ²'s templates (the results files store neither prompt):
`llm_agent` instruction + policy (`reconstructed`); `llm_agent_solo` + the
ticket (`reconstructed`); `llm_agent_gt` + resolution steps rendered from
the task's actions with an approximation of Python's `str()`
(`synthetic`); any other implementation gets the `llm_agent` prompt
(`synthetic`). The user simulator: the guidelines without
`<PERSONA_GUIDELINES>`, then `str(UserScenario)` with `textwrap.indent`
reproduced (`reconstructed`).

### Times (`time.rs`)

`datetime.isoformat()` text read as UTC: an optional `Z` or `+00:00`
suffix, any fraction length (truncated to microseconds), then crosstalk-spec's
RFC 3339 checks (fields in range, no leap second, not before 1970), with
its civil-date arithmetic.

### Ids

Exchange ids derive from the dataset id, the source reference and the
recorded time (the corpus builder, as ct-eval). Labels are `t<n>`, `n`
being the label's index in ct-eval's truth for the world (record order),
the golden export's numbering.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/datasets/tau2/src/lib.rs` | crate root, constants | `DATASET`, `VERSION`, `AGENT`, `USER` |
| `src/options.rs` | selection flags and their manifest record | `Options` (`limit`, `include`, `settings`) |
| `src/source.rs` | the dataset as a `TraceSource` | `source`, `Tau2Source` (`files`, `files_read`) |
| `src/convert.rs` | one simulation to one world | `load_results`, `convert_simulation` |
| `src/error.rs` | the converter's errors | `Tau2Error` |
| `src/files.rs` | file discovery, simulation picks | `discover`, `quota`, `pick`, `world_name` |
| `src/schema.rs` | the results JSON | `Results`, `Info`, `Task`, `UserScenario`, `Instructions`, `Simulation`, `RawMessage`, `RawCall` |
| `src/prompts.rs` | system prompt reconstruction | `agent_system_prompt`, `user_system_prompt`, `scenario_text`, `AGENT_INSTRUCTION` |
| `src/time.rs` | ISO times | `parse_time`, `TimeError` |
| `src/views.rs` | each side's messages | `view`, `View`, `Side`, `Entry` |
| `src/truth.rs` | labels | `SimulationLabels`, `Participant` |
| `tests/tau2.rs` | ct-eval's tests on bench types, plus time checks, ids, numbering, selection, export | |
| `tests/parity.rs` | every fixture label equals ct-eval's, field by field | |
| `tests/fixtures/*.json` | ct-eval's synthetic results files (generated, not dataset bytes) | |
| `tests/fixtures/crosstalk-7f8a2fb-truth.jsonl` | `ct-eval truth --dataset tau2 --root tests/fixtures` at 7f8a2fb | |

## Invariants and constraints

- **Parity.** Same worlds in the same order, same exchanges, same labels
  as ct-eval 7f8a2fb, except the differences below; checked on the
  fixtures and on real data.
- **Times are recorded, never paced**; each agent's exchanges strictly
  increase (1 µs bumps), and a sender's turn precedes the reader's next
  call in every real simulation.
- **Hard-coded turns are not calls.** A record without `raw_data` (the
  agent's greeting) makes no exchange; its text, read by the peer, is a
  boilerplate control.
- **Coverage** `complete {structural}`: the turns are the only way the two
  sides talk.
- **Streaming.** One results file is held at a time.

## Known ct-eval quirks kept

- The user simulator is declared when the implementation is not
  `dummy_user` or any record is a user turn, so a solo run with a stray
  user record gets a user agent.
- A missing model name is `unknown`.
- `limit` takes `ceil(limit / files)` picks per file and stops overall at
  `limit`, so late files may get none (`--limit 200` over 26 files reads 25).
- An unknown finish reason is `other`.
- A turn relays its prompt when its collapsed text is at least 24 bytes
  and occurs in the sender's collapsed prompt.
- A tool result's requestor defaults to `assistant`.
- Turns the peer never answers (`###STOP###`) get no label.
- Shared-source controls are written only when both sides exist.

## Differences from ct-eval

| Difference | Why |
| --- | --- |
| A shared-source control on a tool result no exchange of its reader carries (read after the reader's last call) is left out; its `t<n>` stays unused. Real data: none (all 10,832 simulations); the synthetic fixture has one. | A bench location names an exchange carrying its message; ct-eval's control names only the message. The golden export refuses such worlds (`Gap::UncarriedLocation`); a control no exchange carries guards no prediction. |
| Such a control's place, when a reader exchange carries the message, is the first one. | The golden export's placement; `reader_exchange` stays absent. |
| Labels have ids `t<n>`. | The format needs ids; the golden export's numbering. |
| No token usage, protocol or transport on exchanges. | The format has no such fields. |
| Message ids are the bench's. | By design. |
| The source digest covers the results files read. | The brief's `FilesRead`; the golden export digests every file under the dataset directory. |

## Real-data comparison (ct-eval 7f8a2fb release vs this port)

Dataset `~/Data/ai/agents/tau2-bench` at `5bfa7e37`. Counts only. Labels
are compared field for field by id; exchange ids on the set every label
names.

| | `--limit 200` ct-eval | port | airline in full (`--include airline`, 4 files) ct-eval | port |
| --- | ---: | ---: | ---: | ---: |
| worlds (labelled worlds' keys in the same order) | 200 | 200 | 800 | 800 |
| agents | 336 | 336 | 1,600 | 1,600 |
| exchanges | 4,255 | 4,255 | 14,072 | 14,072 |
| exchange ids named by labels (identical sets) | 3,419 | 3,419 | 14,072 | 14,072 |
| transmissions, direct, user_turn, exact, structural | 1,957 | 1,957 | 9,677 | 9,677 |
| boilerplate controls | 178 | 178 | 1,010 | 1,010 |
| shared-source controls | 1,527 | 1,527 | 5,829 | 5,829 |
| labels differing in any field | | 0 | | 0 |
| failures | 0 | 0 | 0 | 0 |

Whole dataset (26 files, 10,832 simulations): 264,793 exchanges each;
119,256 transmissions, 9,505 boilerplate and 96,960 shared-source controls
each; 211,982 label-named exchange ids, identical; no field differences;
nothing left out.
