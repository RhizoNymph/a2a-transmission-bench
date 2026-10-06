# Dataset `swe_splice` (`a2a-bench-dataset-swe-splice`)

Splices: one channel transmission planted in two real, unrelated Open-SWE
trajectories. A writes a file; B later reads it through its own harness (an
editor view or a shell `cat -n`). It is crosstalk-eval's `swe_splice`
generator (crosstalk 7f8a2fb, `src/datasets/swe_splice/`) on the bench's
types, reading Open-SWE through `a2a-bench-dataset-open-swe`. Version 1
(`swe_splice@1`) reproduces ct-eval's corpus exactly.

## Scope

- A pool of Open-SWE trajectories read round-robin from the selected shards.
- Finding whole-file writes (editor `create`, shell `cat` heredocs),
  working directories and insertion points.
- Planning splice `n` from a seeded stream, and building its world: the
  sender moved into the reader's working directory, the read spliced into
  the reader, both clocks offset, one transmission label and the
  background controls.
- The four variants (`exact`, `whitespace`, `json_string`, `base64`) and
  the need each implies per read form.

## Non-scope

- Detector behaviour (the reference's shell-read routing gap, crosstalk's
  L5 extractors): ct-eval's tests of those are not ported (see below).
- Dataset bytes: tests read the open-swe crate's synthetic shards.

## Selection (`Options`, ct-eval's flags and defaults)

| Field | ct-eval flag | Default | Manifest `selection` key |
| --- | --- | --- | --- |
| `limit` | `--limit` (shards) | all | `limit`, when set |
| `include` | `--include` | all | `include` (JSON array text), when not empty |
| `count` | `--count` (splices) | 40 (`SPLICES`) | `count`, always |
| `seed` | `--corpus-seed` | 0 | `corpus_seed`, always |

`source(root, &options, pace)`: ct-eval seeded its pace with the same
`--corpus-seed`, so ct-eval's corpus for seed `s` is
`Pace::new(1 s, 5 s, s)` (`Pace::DEFAULT` for `s = 0`).

## Data and control flow

```text
source(root, options, pace) ─▶ SpliceSource::open: open_swe::discover

TraceSource::worlds()
   pool(): size = max(3 × count, 16); per shard = ceil(size / shards);
           RoundRobin rows ─▶ Pooled { shard, row, record, workdir }
           (a row error fails the pool: one Err, no worlds); shards opened
           are recorded (FilesRead)
   for n in 0..count:
     plan(pool, n, seed)   SplitMix64::derived(seed, "splice/<n>"):
        sender = pick(trajectories with ≥ 1 splicable write)   NoWriter
        write  = index(its splicable writes)
        reader = pick(others of another repo with an insertion point) NoReader
        insert_at = pick(insertion_points(reader))
        call_id = "chatcmpl-tool-<16 hex of next_u64>"
        variant = ALL[n mod 4]
     world(pool, plan, pace)
        form = ReadForm::of(reader): editor view (str_replace_editor seen;
               `OBSERVATION:` prefix when most results have it) or shell cat
        sent = sender messages with workdir(sender) → workdir(reader)
        written = the same write found again in `sent` (same call, same
                  position among that call's writes)
        body = variant.render(written.content)
        read = reader messages + [form.call(path, id), form.result(…)]
               inserted at insert_at; (start, end) = the numbered lines
        a0, b0 so that read call = write call + 1 on the shared step count
        sender calls: open_swe::calls(sent, |i| pace.at(a0 + i, 1, 0))
        reader calls: open_swe::calls(read, |j| pace.at(b0 + j, 0, 0))
        BackgroundWorld "splice-NNNN-<variant>-<form>":
          agents sender/<harness>/<model>/<dataset>/<row>, reader/…
          label splice/<n>: from sender, to reader,
            sender_exchange = the writing call, reader_exchange = the call
            right after the read (first to carry its result),
            route channel {file: path}, carrier tool_result,
            content = bytes start..end of part 0 of the result message,
            needs/tier = variant.need(form), source {reader shard,
            /rows/<r>/messages/<insert_at+1>/splice/<n>/from/<sender shard>/rows/<s>/messages/<m>}
          controls for every other (sender, reader exchange) pair;
          coverage complete {construction}
```

### Needs (ct-eval's table, kept exactly)

| Variant | Editor view | Shell cat (JSON output) |
| --- | --- | --- |
| exact | `exact` | `decoded [json_string]` |
| whitespace | `normalized` | `decoded [json_string]` |
| json_string | `decoded [json_string]` | `decoded [json_string]` |
| base64 | `decoded [base64]` | `decoded [json_string, base64]` |

All construction tier, in reach. A `json_string` file read through a shell
looks like two string levels from the reader's text, but one is the
writer's: the content sat escaped in the writer's JSON arguments, and
crosstalk's L4 cuts the writer's spans per argument value (INV-1057). So
only one level is the reader's, and the label is in reach. Two levels are
out of reach for ct-eval's reference matcher, not for the spec: the tier is
not `out_of_reach`.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/datasets/swe-splice/Cargo.toml` | package `a2a-bench-dataset-swe-splice`; open-swe, corpus (`parquet`), base64 | |
| `src/lib.rs` | crate root, constants, `source` | `DATASET` (`swe_splice`), `VERSION` (1), `SPLICES` (40), `MIN_CONTENT` (160), `MAX_CONTENT` (16,000), `DEFAULT_WORKDIR` (`/testbed`), `source` |
| `src/options.rs` | selection flags, manifest form, pool size | `Options` (`settings`, `pool_size`) |
| `src/error.rs` | typed errors | `SpliceError` (`OpenSwe`, `NoWriter`, `NoReader`, `Range`, `Text`, `OutOfText`, `Corpus`) |
| `src/path.rs` | lexical path normalization (ct-eval's reference `normalize_path`) | `normalize_path` |
| `src/write.rs` | whole-file writes in a trajectory | `writes`, `heredocs`, `resolve`, `FileWrite`, `WriteForm` |
| `src/read.rs` | the reader's harness read | `ReadForm` (`of`, `name`, `call`, `result`), `numbered`, `view_banner`, `OBSERVATION` |
| `src/variant.rs` | the four variants and their needs | `Variant` (`ALL`, `name`, `render`, `need`), `perturb` |
| `src/pool.rs` | pooled trajectories, workdirs, insertion points | `Pooled` (`new`, `writes`), `splicable`, `workdir`, `rewrite_workdir`, `insertion_points`, `calls_before` |
| `src/plan.rs` | planning splice `n` | `Plan`, `plan` |
| `src/world.rs` | the world of a plan | `world` |
| `src/source.rs` | the `TraceSource` | `SpliceSource` (`open`, `shards`, `pool`, `files_read`) |
| `tests/swe_splice.rs` | ported ct-eval tests, selection, conversion of the spliced read, pool files, export determinism | |

## Invariants and constraints

- **Parity** with ct-eval for the same root, selection, seed and pace
  (below).
- **One planted transmission per world**, sender and reader of different
  repositories; the planted (sender, reader exchange) has no control, so a
  mis-routed prediction there is a plain false positive.
- **Clock.** The read call is one paced step after the writing call and the
  reader exchange one step later still: the result arrives 2 to 10 s after
  the write with the default pace (tested), inside crosstalk's live 60 s
  correlation window.
- **Only the reader exchange first carries the result**; earlier reader
  exchanges do not contain its message (tested).
- **Deterministic**: splice `n` draws from `derived(seed, "splice/<n>")`;
  shards sorted; reruns identical; exports byte-identical (tested).
- **Files read**: the shards the pool opened (`pool()` runs inside
  `worlds()`; read the source to the end, or call `pool()`, before
  digesting).

## ct-eval quirks kept

- **Exchange ids repeat across worlds.** A trajectory pooled into several
  splices at the same clock offset derives the same ids (dataset + source +
  time): at `--count 80`, 11,780 exchanges carry 3,868 distinct ids. Ids are
  unique within a world, which is all the format checks.
- Every reader comes from another repository, so splice worlds hold only
  `boilerplate` controls.
- The workdir rewrite is a plain substring replacement in every text,
  reasoning and argument of the sender.
- The heredoc parser handles `cat > P <<D`, `cat <<D > P`, `<<-`, quoted
  delimiters and a `cd DIR &&` prefix; `>>` appends are skipped; an
  unclosed heredoc stops the scan of that command.
- The spliced call has empty `content` and `reasoning_content` (so the
  assistant message holds only the tool call).
- The label's source path is the composite
  `/rows/…/splice/<n>/from/<shard>/rows/…/messages/…`.
- A pool row error fails the whole corpus (one failure, no worlds).
- `Plan` failures (`NoWriter`, `NoReader`) fail that world only.

## Differences from ct-eval

- The label id `splice/<n>` (ct-eval's labels had none); `exchange_agent`
  rows from the builder.
- The route's resource is the bench's `file {path}`; ct-eval's was
  `Locator::File { host: None, path }`.
- `world` takes the pace (ct-eval had `world` at the default pace and
  `world_paced`).
- Index panics ct-eval could not reach are typed errors.

### ct-eval tests not ported

`the_reference_finds_editor_view_splices` (the reference matcher, another
crate) and `the_real_extractor_reads_an_editor_view_as_the_file` /
`the_real_extractor_reads_a_shell_cat_as_the_same_file` (crosstalk-flow's
L5 extractors; the bench must not depend on crosstalk). Their corpus side,
the exact call and result the splice inserts, is pinned by
`spliced_reads_convert_to_a_call_and_its_result`. The whitespace variant's
fold check uses a local whitespace fold instead of the reference's.

## Real-data comparison (crosstalk 7f8a2fb release `ct-eval`, 2026-10-05)

`--count 80`, seed 0 (`Pace::DEFAULT`), all 13 shards.

| | ct-eval | bench |
| --- | --- | --- |
| worlds (same keys, same order) | 80 | 80 |
| agents | 160 | 160 |
| exchanges | 11,780 | 11,780 |
| distinct exchange ids (equal sets) | 3,868 | 3,868 |
| controls `boilerplate` / structural | 11,700 | 11,700 |
| transmissions, channel `file`, `tool_result`, construction | 80 | 80 |
| · `exact` | 14 | 14 |
| · `normalized` | 15 | 15 |
| · `decoded [json_string]` | 31 | 31 |
| · `decoded [base64]` | 16 | 16 |
| · `decoded [json_string, base64]` | 4 | 4 |

All 11,780 labels matched as whole tuples (world, agents, sender and reader
exchanges, route and path, carrier, needs, tier, source, part, byte range
and content text); none on one side only.
