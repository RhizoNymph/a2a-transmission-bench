# Corpus model and export writer (`a2a-bench-corpus`)

The crate every dataset converter builds on (`crates/corpus`). It is
crosstalk-eval's corpus model (world builder, virtual clock, trace sources)
ported onto the bench's format types, plus what ct-eval did not have: the
export writer, the detector's input view, `datasets.toml` handling and the
dev/holdout splits of design §9.2. It depends only on `a2a-bench-format`,
never on a crosstalk crate.

## Scope

- `WorldBuilder`: declaring agents (model-driven or scripted), adding
  exchanges from drafts, adding labels, finishing a world that has passed
  the format's checks (`WorldInputs::new`, `check_labels`).
- The synthetic client: a stable credential fingerprint per agent (ct-eval's
  digest), vendor from the model name, optional session and turn.
- The virtual clock `Pace` (`compose`, `ordinal`), bit for bit ct-eval's.
- `TraceSource`: a dataset as a stream of finished worlds.
- `export`: worlds to `messages.jsonl`, `exchanges.jsonl`, `labels.jsonl`
  and `manifest.json`; the source digest; the input view.
- `datasets.toml` (data root, dataset paths, pinned revisions).
- Splits: dev lists, holdout complement, release-only holdout exports.
- Generic converter helpers: OpenAI-chat messages, background worlds of
  independent agents, SplitMix64, `new_inputs`, and typed Parquet rows
  (feature `parquet`).

## Non-scope

- Converters themselves (`a2a-bench-datasets`), resource canonicalisation
  (`a2a-bench-resource`), scoring, the CLI.
- Revision pinning policy (`--allow-revision`): the config exposes the
  pinned revision; the CLI enforces it.
- The holdout commitment file (`splits/<dataset>@<n>.holdout.commit`) and
  aggregate-only holdout reports: the scorer and CLI.
- Real split files: none exist yet.
- Dataset bytes: tests use synthetic worlds only.

## Data and control flow

```text
converter
  WorldBuilder::new(dataset, world)
    model_agent / model_agent_with / scripted_agent ──▶ WorldAgent (world + key)
    exchange(ExchangeDraft) ─┬─ agent of this world, declared, model-driven
                             ├─ at > the agent's previous exchange
                             ├─ id = format::ids::exchange_id(dataset, source, at)
                             │   (recorded_exchange keeps a dataset's id)
                             ├─ id not seen before
                             └─ Client {credential, session, turn, vendor, model};
                                messages stored once per world
    label(Label)             ─ exchange_agent rows refused (the builder's)
    finish(coverage) ── sort exchanges (at, agent, id)
                     ── exchange_agent row per exchange, then the labels
                     ── World::new: WorldInputs::new + check_labels ──▶ World

TraceSource::worlds() ──▶ Result<World, S::Error>, one at a time

export(source, out_dir, ManifestInfo, &Selection)
  refuse: source dataset ≠ info dataset; split keys in info.selection;
          list revision ≠ info.source.revision; holdout inside a git repo;
          out_dir not empty
  source.select(filter)               (hint; sources may skip parsing)
  for each world:
    Err  ──▶ tracing warn + WorldFailure {position, error}; go on
    not kept by the filter ──▶ left_out += 1
    kept ──▶ messages  {world key}   messages in first-use order
             exchanges {decl}        exchanges in time order
             labels    {key, coverage} labels
  trailers ──▶ Manifest {…, split, selection + split settings, worlds, files}
           ──▶ manifest.json (pretty JSON, final newline)
  ──▶ Exported {manifest, failures, left_out, missing}

input_view(export_dir, dest)
  manifest.json ──▶ Manifest::input_view() ──▶ dest/manifest.json
  messages.jsonl, exchanges.jsonl ──▶ hard link (copy on failure)
  labels.jsonl never
```

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `src/lib.rs` | crate root | modules |
| `src/world/mod.rs` | the checked world and the corpus errors | `World` (`new`, `key`, `decl`, `inputs`, `exchanges`, `exchange`, `message`, `messages_in_order`, `labels`, `agent_of`, `coverage`), `CorpusError` |
| `src/world/builder.rs` | assembling a world | `WorldBuilder` (`new`, `model_agent`, `model_agent_with`, `scripted_agent`, `exchange`, `recorded_exchange`, `label`, `finish`), `ExchangeDraft`, `WorldAgent` |
| `src/world/client.rs` | synthetic credential, vendor | `Credential` (`synthetic`, `of_digest`, `recorded`), `credential_digest`, `vendor_of`, `CREDENTIAL_PREFIX` |
| `src/world/stop.rs` | stop reasons as written | `StopReason` (`as_str`, `for_calls`) |
| `src/clock.rs` | the virtual clock | `Pace` (`DEFAULT`, `new`, `at`, `min`, `max`, `seed`, `settings`), `compose`, `ordinal`, `EPOCH_MICROS`, `MIN_STEP`, `MINOR_LIMIT`, `SUB_LIMIT`, `ClockError` |
| `src/source.rs` | streaming sources | `TraceSource` (`Error`, `dataset`, `select`, `worlds`), `WorldFilter`, `InMemory` |
| `src/delta.rs` | what a request adds to the agent's previous one | `new_inputs` |
| `src/export/mod.rs` | the export writer | `export`, `ManifestInfo`, `Exported`, `WorldFailure`, `ExportError` |
| `src/export/write.rs` | the three files in lockstep, the manifest file | `MANIFEST_FILE`, `MESSAGES_FILE`, `EXCHANGES_FILE`, `LABELS_FILE`, `write_manifest`, `read_manifest` |
| `src/export/digest.rs` | the source digest | `source_digest`, `FilesRead`, `SOURCE_DIGEST_CONTEXT`, `DigestError` |
| `src/export/input_view.rs` | the detector's input dir | `input_view` |
| `src/split/mod.rs` | selections and releases | `Selection` (`Unsplit`, `Dev`, `Holdout`; `dev`, `holdout`, `split`, `filter`, `settings`, `check`), `Release`, `SplitError`, `SPLIT_LIST_KEY`, `SPLIT_DIGEST_KEY`, `RELEASE_KEY` |
| `src/split/list.rs` | dev list files | `DevList` (`load`, `parse`, `worlds`, `revision`, `digest`), `dev_list_path` |
| `src/split/repo.rs` | git repository detection | `enclosing_repository` |
| `src/config.rs` | `datasets.toml` | `DatasetsConfig` (`load`, `parse`, `dataset`, `root`, `dataset_root`), `DatasetConfig` (`pinned_revision`), `expand`, `home`, `DEFAULT_ROOT`, `DEFAULT_DATASETS`, `ConfigError` |
| `src/helpers/chat.rs` | OpenAI-chat messages to bench bodies | `ChatMessage`, `ChatToolCall`, `ChatFunction`, `bodies`, `convert`, `arguments`, `synthetic_call_id`, `null_as_default`, `ChatError` |
| `src/helpers/background.rs` | worlds of independent agents with controls | `BackgroundWorld`, `Trajectory`, `Call` |
| `src/helpers/rng.rs` | seeded generator | `SplitMix64` |
| `src/helpers/parquet_rows.rs` | typed Parquet rows (feature `parquet`) | `ParquetRows`, `row_groups`, `ParquetError` |
| `/datasets.toml` | the repo's dataset paths, revisions empty | |
| `tests/clock.rs` | pace bounds, order, determinism, crosstalk parity | |
| `tests/corpus.rs` | builder refusals, clients, ordering, new inputs | |
| `tests/truth.rs` | labels through the builder and an export | |
| `tests/export.rs` | lockstep files, manifest, determinism, failures, input view | |
| `tests/split.rs` | dev lists, selection logic, dev and holdout exports | |
| `tests/config.rs`, `tests/helpers.rs` | config, chat, background, rng, source digest | |
| `tests/fixtures/crosstalk-7f8a2fb-corpus-vectors.json` | `Pace::at`, `compose`, `ordinal`, credential digests and vendors computed by crosstalk at 7f8a2fb | |
| `tests/fixtures/crosstalk-corpus-vectors-generator.rs.txt` | the program that computed them | |

## Invariants and constraints

- **A world is valid or does not exist.** `World::new` (and so
  `WorldBuilder::finish`) runs `WorldInputs::new` and `check_labels`, the
  checks every reader applies. The builder also refuses, as ct-eval's did:
  an undeclared, foreign (another world's `WorldAgent`) or scripted agent;
  an exchange not later than its agent's previous one; a duplicate exchange
  id; and a converter-supplied `exchange_agent` row.
- **Ids.** Derived exchange ids are `a2a_bench_format::ids::exchange_id`
  of the dataset, the draft's `SourceRef` and its time, so they equal
  ct-eval's. Label ids are the converter's; `BackgroundWorld` names its
  controls `bg/<sender>/<reader exchange>`.
- **The clock is ct-eval's.** `Pace::at`, `compose`, `ordinal`, the
  constants and the credential digest equal crosstalk's on every vector of
  the fixture. One deliberate difference: when `major × mean + jitter`
  overflows u64 (majors above about 6.1e12, unreachable), ct-eval's release
  build wraps and its debug build panics; the bench returns
  `ClockError::Major`. No vector differs.
- **Credentials.** `k:` + hex of
  `BLAKE3("crosstalk-eval/v1" 0 "credential" 0 dataset 0 world 0 agent)`.
  crosstalk's adapter strips `k:`, parses the hex and rebuilds ct-eval's
  `CredentialHash::from_keyed_digest(SecretVersion(0), digest)`. Vendors
  are `anthropic`, `google`, `openai` or the model's first path segment
  (ct-eval's `Vendor::OpenAi` is `openai`, `Other(p)` is `p`), taken from
  the agent's declared model.
- **Determinism.** Agents are declared in key order; exchanges sorted by
  (time, agent, id); messages in first-use order; every map ordered;
  the manifest is pretty JSON with a final newline. Two exports of the same
  source, info and selection are byte-identical (tested).
- **Streaming.** One world is held at a time; the three files are written
  in lockstep so their sections line up.
- **Failures do not stop an export.** A world the source fails to produce
  is logged (`warn`, with dataset, position, error) and returned in
  `Exported::failures`. I/O and write errors, and a world of another
  dataset, are fatal.
- **The input view never holds `labels.jsonl`**, and its manifest has no
  labels digest (the manifest digest is unchanged by that).
- **Splits.** A dev list applies only to its source revision. A holdout
  needs a dev list and a `Release`, and is refused inside any git
  repository (a `.git` directory or file at or above the nearest existing
  ancestor of the output). The manifest records `split`, and in
  `selection`: `split_list` (the list's file name, or `none` for an unsplit
  dataset), `split_digest` (BLAKE3 of the list file) and, for a holdout,
  `release`. Converters may not use those keys.
- **Source digest.** BLAKE3 derive-key `a2a-bench/1 source` over each file
  in path order: `path 0x00 len_u64_le contents`, paths relative to the
  dataset root, `/`-separated.
- No `unsafe`; no unwrap/expect/panic outside tests; errors are
  `thiserror` enums.

## How to add a converter

1. Add the converter in `a2a-bench-datasets` (`src/<name>/`) with a source
   type implementing `TraceSource`: `type Error` is the converter's own
   error enum, `worlds()` yields one `World` per independent set of agents,
   reading files lazily, one world per `next`, in a deterministic order. If
   world keys are known before parsing (file stems), honour
   `TraceSource::select` to skip worlds the export will not keep.
2. Per world, use `WorldBuilder`:
   - `model_agent(name, model)` or `scripted_agent(name)` per agent
     (`model_agent_with` when the dataset records a credential);
   - `exchange(ExchangeDraft { … })` per model call, in any order, with
     every message built by `Message::new(body)` (or
     `helpers::chat::convert` for OpenAI-chat records), following the
     format's normalisation rules (tool results as their own `tool`
     messages, system messages inline);
   - `label(…)` per label;
   - `finish(Coverage::…)`, which fails if anything does not check.
3. Times: if the dataset has none, compose them with `clock::Pace` (or
   `compose` for the default pace) from whatever orders its records, and
   record `pace.settings()` in the manifest. Each agent's exchanges must
   strictly increase, and a sender's exchange must precede the reader's.
4. Source references: give every exchange and label a `SourceRef` (file
   relative to the dataset root, JSON-pointer-like path). Exchange ids
   derive from it, so keep ct-eval's references for parity.
5. Labels: give each a unique id, pick the tier honestly, set `needs` to
   the weakest match class the dataset's construction implies, and add
   negative controls for the dataset's known traps. Declare
   `Coverage::Complete` only when every transmission in the world is
   labelled. Independent-trajectory corpora use
   `helpers::background::BackgroundWorld`.
6. Record the files read (`export::FilesRead`) and pass their
   `source_digest` and the dataset's revision in `ManifestInfo.source`.
7. Register the dataset in `datasets.toml` and the CLI, add
   `docs/datasets/<dataset>.md` with its version, synthetic fixtures under
   the datasets crate's `tests/fixtures/<name>/`, and tests in
   `tests/<name>.rs`.
