# Dataset `open_swe` (`a2a-bench-dataset-open-swe`)

Open-SWE-Traces as a background (negative) corpus: many independent
single-agent SWE trajectories mixed into one world. Nothing passed between
them, so every prediction is a false positive and a report gives the
false-positive rate per 1k exchanges. It is crosstalk-eval's `open_swe`
converter (crosstalk 7f8a2fb, `src/datasets/open_swe/`) on the bench's
types. Version 1 (`open_swe@1`) reproduces ct-eval's corpus exactly.

The crate also exports what the `swe_splice` generator reads Open-SWE with
(shard discovery, the round-robin row reader, the call builder).

## Scope

- Discovering shards `data/<harness>/<model>/<dataset>/*.parquet` under the
  dataset root (`datasets.toml`: `open-swe-traces`), filtered and limited.
- Reading rows (projected columns `instance_id`, `repo`, `trajectory_id`,
  `messages`) one shard after another in turn, at most `count` per shard.
- Converting each trajectory's OpenAI-chat messages
  (`a2a_bench_corpus::helpers::chat`) and cutting them into calls.
- Mixing `agents_per_world` trajectories into background worlds
  (`a2a_bench_corpus::helpers::background`).
- Recording the shards opened, for the source digest.

## Non-scope

- Positives: they come from `swe_splice`.
- Tool schemas (`tools` column), `resolved`, `license`, `language`,
  `metadata`: not projected, never exported (`request.tools` is absent).
- The CLI, scoring, the reference matcher (other crates).
- Dataset bytes: tests use the synthetic shards in `tests/fixtures/open_swe`.

## Selection (`Options`, ct-eval's flags and defaults)

| Field | ct-eval flag | Default | Manifest `selection` key |
| --- | --- | --- | --- |
| `limit` | `--limit` (shards) | all | `limit`, when set |
| `include` | `--include` (repeatable, substring of the relative path) | all | `include` (JSON array text), when not empty |
| `count` | `--count` (rows per shard) | all | `count`, when set |
| `agents_per_world` | `--agents-per-world` | 16 | `agents_per_world`, always |

The source is paced (`source(root, &options, pace)`). ct-eval seeded the
pace with its `--corpus-seed` (default 0) and had `--pace-min-ms 1000`,
`--pace-max-ms 5000`: `Pace::DEFAULT` is ct-eval's default corpus. The
CLI records `pace.settings()` in the manifest.

## Data and control flow

```text
source(root, options, pace) ─▶ OpenSweSource::open
   files::discover(root, limit, include)
     data/<harness>/<model>/<dataset>/<file>.parquet, each level sorted,
     include = any needle a substring of the relative path, truncate(limit)

TraceSource::worlds()
   RoundRobin::new(shards, per_shard = count)
     next_row(): shard cursor % n; a lane past `count` or exhausted is done;
                 opening a lane records its shard (take_opened → FilesRead);
                 an open/read error is returned once, the lane is then done
   loop per world:
     pending error (FIFO) ─▶ yield Err first
     batch = next_row() until agents_per_world rows (errors queued)
     batch empty ─▶ the queued errors, then the end
     mix("mix-NNNNN", batch, pace)
        BackgroundWorld::new("open_swe", key)
        per slot t: trajectory(shard, row, record, t, pace)
           name  = <harness>/<model>/<dataset>/<row>
           model = <model> (the shard's directory name)
           group = record.repo
           calls(messages, shard.relative, row, |i| pace.at(i, t, 0))
             convert(messages) (tool results paired by position)
             each assistant message i (the c-th): request = messages[..i],
             response = messages[i], stop tool_use/end_turn,
             fidelity reconstructed, source {shard, /rows/<row>/messages/<i>}
        finish(): a control per (sender, reader exchange) of distinct
                  trajectories, shared_source if same repo else boilerplate,
                  tier structural, id bg/<sender>/<exchange>;
                  coverage complete {construction}
```

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/datasets/open-swe/Cargo.toml` | package `a2a-bench-dataset-open-swe`; corpus with feature `parquet` | |
| `src/lib.rs` | crate root, constants, `source` | `DATASET` (`open_swe`), `VERSION` (1), `AGENTS_PER_WORLD` (16), `COLUMNS`, `source` |
| `src/options.rs` | selection flags and their manifest form | `Options` (`settings`), `count_setting`, `include_setting` |
| `src/error.rs` | typed errors | `OpenSweError` (`NoData`, `Io`, `Parquet`, `Chat`, `Clock`, `Corpus`) |
| `src/files.rs` | shard discovery | `Shard` (`parse`), `discover` |
| `src/schema.rs` | the projected row | `OpenSweRow` |
| `src/rows.rs` | rows of each shard in turn, opened shards | `RoundRobin` (`new`, `next_row`, `take_opened`) |
| `src/trajectory.rs` | calls, trajectories, worlds | `calls`, `agent_name`, `trajectory`, `mix` |
| `src/source.rs` | the `TraceSource` | `OpenSweSource` (`open`, `shards`, `files_read`) |
| `tests/open_swe.rs` | ported ct-eval tests plus selection, pace and files-read tests | |
| `tests/fixtures/open_swe/` | six synthetic rows over three harness shards as `rows.jsonl`, the Parquet built from them, `make.sql` (`duckdb < make.sql`, one-off) | |

## Invariants and constraints

- **Parity.** Same worlds (`mix-00000`, … in order), agents, exchanges and
  ids, messages, times and labels as ct-eval for the same root, selection
  and pace (see the comparison below).
- **No positives.** Coverage complete at the construction tier; every
  ordered pair of distinct trajectories has a control at each of the
  reader's exchanges.
- **Clock.** Slot `t`, call `i` at `pace.at(i, t, 0)`: every trajectory
  starts together at the epoch and they interleave call by call. Slots are
  milliseconds (`minor`), so a world holds at most 1,000 trajectories; more
  fails the world with a clock error (as ct-eval).
- **Deterministic.** Shards in sorted path order, rows in file order, one
  per shard in turn. Reruns are identical (tested).
- **Streaming.** One Parquet row is decoded at a time; one world is held.
- **Files read** are exactly the shards opened (relative paths), recorded
  as worlds are produced; read the source to the end before digesting.

## ct-eval quirks kept

- The agent's model is the shard's model directory name (`qwen36_27b`),
  not a recorded model string; the vendor is derived from it.
- Tool messages carry no `tool_call_id`: each answers the oldest unanswered
  call before it; a result with no open call gets `unpaired-<index>`, a
  call without an id `call-<message>-<call>`. Outcomes are `success`.
- `include` matches substrings of the whole relative path, so `sweagent`
  also picks `minisweagent` shards.
- A failing shard yields one error, queued and returned before the next
  world (FIFO), and is skipped from then on.
- `agents_per_world = 0` is treated as 1.
- A trajectory with no assistant message adds an agent with no exchanges.

## Differences from ct-eval

- Bench label ids (ct-eval had none): controls are `bg/<sender>/<reader
  exchange>` (the corpus helper); the builder writes `exchange_agent` rows.
- No `WireProtocol` (OpenAI chat) on exchanges and no tool-call signatures:
  the bench format has neither.
- Unreachable panics are typed errors (none occur on the data).

## Real-data comparison (crosstalk 7f8a2fb release `ct-eval`, 2026-10-05)

`--count 16`, defaults otherwise (13 shards, 16 per world, pace seed 0).
ct-eval's side: `ct-eval truth` labels and the `run --detector reference`
report totals. Bench side: `OpenSweSource` with `Pace::DEFAULT`.

| | ct-eval | bench |
| --- | --- | --- |
| worlds (same keys, same order) | 13 | 13 |
| agents | 208 | 208 |
| exchanges | 14,311 | 14,311 |
| distinct exchange ids (equal sets) | 14,311 | 14,311 |
| controls `boilerplate` / structural | 214,328 | 214,328 |
| controls `shared_source` / structural | 337 | 337 |
| transmissions | 0 | 0 |

Every label matched as a whole tuple (world, from, to, reader exchange,
reason, tier, source file and path): 214,665 identical, none on one side
only.
