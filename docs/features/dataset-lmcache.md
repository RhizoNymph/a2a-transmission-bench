# Dataset `lmcache` (`a2a-bench-dataset-lmcache`)

LMCache agentic traces as a background (negative) corpus: independent
agent sessions mixed into worlds, every prediction a false positive. It is
crosstalk-eval's `lmcache` converter (crosstalk 7f8a2fb,
`src/datasets/lmcache/`) on the bench's types. Version 1 (`lmcache@1`)
reproduces ct-eval's corpus exactly.

## Scope

- The data files `data/*.parquet` under the dataset root (`datasets.toml`:
  `lmcache`), their row groups, and whole sessions read from every row
  group in turn (projected columns `session_id`, `model`, `input`,
  `pre_gap`).
- Rebuilding calls from cumulative requests: a request's response is the
  assistant message the session's next request appends.
- The dataset's own clock (summed `pre_gap`s); no pace.
- Mixing `agents_per_world` sessions into background worlds.

## Non-scope

- `output_length` (not projected), positives, the CLI, scoring.
- Dataset bytes: tests use `tests/fixtures/lmcache` (synthetic).

## Selection (`Options`, ct-eval's flags and defaults)

| Field | ct-eval flag | Default | Manifest `selection` key |
| --- | --- | --- | --- |
| `limit` | `--limit` (files) | all | `limit`, when set |
| `include` | `--include` (substring of `data/<file>`) | all | `include` (JSON array text), when not empty |
| `count` | `--count` (sessions per file) | all | `count`, when set |
| `agents_per_world` | `--agents-per-world` | 16 | `agents_per_world`, always |

`source(root, &options)` takes no pace: LMCache keeps its recorded times,
and the manifest's `pace` is empty.

## Data and control flow

```text
source(root, options) ─▶ LmcacheSource::open
   files::discover: data/*.parquet (one level), include, sorted, truncate(limit)

TraceSource::worlds()
   record every selected file (FilesRead)
   files::segments: row-group counts of each file; interleaved
     [(f0,g0), (f1,g0), …, (f0,g1), …]   (an error is queued)
   Sessions::new(segments, per_file = count)
     next(): segment cursor % n; a file past `count` sessions is done;
       read(): open the row group (ParquetRows::open_group, file row numbers);
               take rows while session_id is unchanged (Parquet row order);
               the first session of a group > 0 is skipped (it may continue
               the previous group's last session)
   loop per world:
     pending error (a stack: the latest first) ─▶ yield Err
     batch = sessions until agents_per_world (errors pushed)
     mix("mix-NNNNN", batch)
        per session: trajectory(session)
          name = session id; model = first row's model ("unknown" if none);
          group = group(id): swebench__<repo>__… → swebench/<repo>,
                  <kind>__<task>__… → <kind>/<task>, else the id
          calls(session): for each (row r, row r+1):
            elapsed += max(pre_gap_r, 0) (missing = 0)
            reply = first assistant message of input_{r+1} past len(input_r)
              none ─▶ call dropped (debug)
            request = convert(input_r); response = convert(input_{r+1})[reply]
            fidelity reconstructed if input_{r+1} starts with input_r, else
              synthetic (debug)
            at = EPOCH + round(elapsed × 1e6) µs, bumped 1 µs past the
                 previous call when not later
            stop tool_use/end_turn; source {file, /rows/<r>}
          the last request has no response: dropped
        BackgroundWorld finish: controls per (sender, reader exchange),
          shared_source within a group, boilerplate otherwise;
          coverage complete {construction}
```

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/datasets/lmcache/Cargo.toml` | package `a2a-bench-dataset-lmcache`; corpus with feature `parquet` | |
| `src/lib.rs` | crate root, constants, `source` | `DATASET` (`lmcache`), `VERSION` (1), `AGENTS_PER_WORLD` (16), `COLUMNS`, `source` |
| `src/options.rs` | selection flags, manifest form | `Options` (`settings`) |
| `src/error.rs` | typed errors | `LmcacheError` (`NoData`, `Io`, `Parquet`, `Chat`, `Time`, `Corpus`) |
| `src/files.rs` | files and row groups | `discover`, `Segment`, `segments` |
| `src/schema.rs` | the projected row | `LmcacheRow` |
| `src/sessions.rs` | whole sessions from each segment in turn | `Session`, `Sessions` |
| `src/calls.rs` | calls, trajectories, worlds, groups | `group`, `calls`, `trajectory`, `mix` |
| `src/source.rs` | the `TraceSource` | `LmcacheSource` (`open`, `files`, `files_read`) |
| `tests/lmcache.rs` | ported ct-eval tests plus selection and files-read tests | |
| `tests/fixtures/lmcache/` | two synthetic files (the first with two row groups and a session crossing them) as JSONL, the Parquet built from them, `make.sql` | |

## Invariants and constraints

- **Parity** with ct-eval for the same root and selection (below).
- **No positives**, complete coverage at the construction tier.
- **Parquet row order is relied on**: a session's rows are contiguous and
  in request order. The reader never sorts.
- **Times** are the dataset's: strictly increasing per session (ties bumped
  by 1 µs), every session starting at the corpus epoch.
- **Bounded memory**: one row group cursor per segment, rows decoded as
  consumed; one world held at a time.
- **Files read**: every selected file (their row-group metadata is read
  before any world).

## ct-eval quirks kept

- Errors are returned last-in first-out (open-swe's are FIFO).
- The first session of every row group after the first is skipped, whether
  or not it continues the previous group's last session; a session that
  crosses a boundary is cut at it.
- `count` limits sessions per file, across that file's row groups.
- A call whose next request appends no assistant message is dropped; a
  session's last request always is.
- Negative or missing `pre_gap` counts as 0; seconds are rounded to whole
  microseconds (the float-to-integer conversion saturates).
- Tool results keep their recorded `tool_call_id`s (no positional pairing
  needed, though the helper would apply it to a result without one).

## Differences from ct-eval

- Bench label ids and `exchange_agent` rows (as for open-swe).
- A time past `u64` microseconds is `LmcacheError::Time` (ct-eval added
  unchecked); unreachable on the data.

## Real-data comparison (crosstalk 7f8a2fb release `ct-eval`, 2026-10-05)

`--count 16`, defaults otherwise (5 files, 16 sessions per world).

| | ct-eval | bench |
| --- | --- | --- |
| worlds (same keys, same order) | 5 | 5 |
| agents | 80 | 80 |
| exchanges | 2,513 | 2,513 |
| distinct exchange ids (equal sets) | 2,513 | 2,513 |
| controls `boilerplate` / structural | 31,660 | 31,660 |
| controls `shared_source` / structural | 6,035 | 6,035 |
| transmissions | 0 | 0 |

All 37,695 labels matched as whole tuples (world, from, to, reader
exchange, reason, tier, source); none on one side only.
