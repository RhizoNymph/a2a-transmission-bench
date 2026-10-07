# The `a2a-bench` command line (`a2a-bench-cli`)

The binary that ties the bench together (`crates/cli`, binary `a2a-bench`):
it exports datasets through the converter crates, checks exports and
predictions files, builds a detector's input view, runs a detector process,
scores its predictions and diffs exports or predictions for parity. Design:
[../design/separation.md](../design/separation.md) §2, §4, §5.4, §7, §8, §9.

## Scope

- `export`: dispatch to each converter crate's `source(...)` with its
  `Options` built from ct-eval's flags and defaults; `corpus::export`; the
  source digest of the files read; converter version and bench commit;
  source revision detection (HF snapshot, HF `--local-dir` metadata, git
  HEAD, else `unknown`) and pinning; dev and holdout splits; the
  holdout commitment file. demo-swarm through its own entry point, dev or
  holdout (a whole run with a holdout seed, committed one line per run).
- `validate`: every format check over an export (or an input view) and,
  optionally, a predictions file. Counts only.
- `input-view`: the detector's input directory.
- `run`: input view, detector process (the contract of design §4), the
  determinism check (`--twice`), score.
- `score`: the scorer with the bench's resource canonicaliser, gates,
  `report.json` / `report.txt`, exit 2 on a failed gate.
- `diff`: two exports or two predictions files, world by world and row by
  row, for parity stages P1–P5. With `--normalize-ids`, predictions'
  detector agents are named by the exchanges they hold and transmissions
  compared without ids (P3).
- Revision spelling: a directory with no revision is `unknown`, as in
  crosstalk's golden export.

## Non-scope

- Detection (detectors are external programs; `a2a-reference` is in
  `crates/reference`).
- Conversion, scoring rules, gates' contents, the format: the library
  crates. The CLI only wires them.
- Real split lists and revision pins: none exist yet (`splits/` holds
  nothing, `datasets.toml` pins nothing).
- `diff --agreement` (design §5.2) and gates keyed by dataset version
  (design §8): not built.
- The demo-swarm runs themselves: node0 makes them and crosstalk's
  adapter writes their captures and predictions. The CLI's demo-swarm
  tests (`tests/demo_swarm_holdout.rs`) build a synthetic capture with the
  crate's test fixture.

## Commands

```text
a2a-bench export --dataset <id> --out <dir> [--root <data root>] [--dataset-dir <dir>]
                 [--config datasets.toml] [--splits <dir>] [--version N]
                 [--split dev|holdout --release <detector>@<tag>] [--allow-revision]
                 [dataset flags…]
a2a-bench export --dataset demo-swarm --inputs <capture dir> --truth <truth.jsonl> --out <dir>
                 [--run-lead-ms N] [--run-slack-ms N]
                 [--split holdout --release <detector>@<version> [--seed N] [--splits <dir>]]
                 (capture dir: messages.jsonl, exchanges.jsonl, manifest.json;
                  bench.env in it or its parent names a holdout run's seed)
a2a-bench validate <export dir> [--predictions <file>]
a2a-bench input-view <export dir> <dest>
a2a-bench run --export <dir> --detector-cmd "<program> [args…]" --out <dir>
              [--gates <path>] [--examples N] [--holdout-release <detector>@<tag>] [--twice]
a2a-bench score --export <dir> --predictions <file> --out <dir>
                [--gates <path>] [--examples N] [--holdout-release <detector>@<tag>]
a2a-bench diff <a> <b> [--normalize-ids] [--first N]
```

Exit codes: 0 success; 1 an error, a failed validation, or an export with
worlds that failed to convert (the rest is written); 2 a failed gate
(`score`, `run`, as ct-eval). `diff` follows diff(1): 0 equal, 1 different,
2 error. Logs are `tracing` key-value lines on stderr (`RUST_LOG`, default
`info`); stdout holds only counts, ids and the report table.

### Datasets and their flags

`--dataset` takes the bench ids; ct-eval's spellings are aliases (`wiki`,
`swarm`, `open-swe`, `swe-splice`). A flag the dataset does not read is
refused. Paced datasets build `Pace::new(--pace-min-ms, --pace-max-ms,
--corpus-seed)` (defaults 1000, 5000, 0: exactly ct-eval's
`open_source`), and record `pace.settings()`; the others keep their own
times and record no pace.

| `--dataset` | Crate | Flags (ct-eval defaults) | Paced |
| --- | --- | --- | --- |
| `salt` | `a2a-bench-dataset-salt` | `--limit`, `--include…` | yes |
| `agentdojo` | `…-agentdojo` | `--limit`, `--include…` (`pipeline=…`, `suite=…`, `attack=…`, `task=…`) | yes |
| `tau2` | `…-tau2` | `--limit`, `--include…` | no |
| `collusion-wiki` (`wiki`) | `…-wiki` | `--limit`, `--family…`, `--wiki…`, `--min-agents`, `--max-agents`, `--demo` | yes |
| `swarm-traces` (`swarm`) | `…-swarm` | `--limit` | yes |
| `open_swe` (`open-swe`) | `…-open-swe` | `--limit`, `--include…`, `--count`, `--agents-per-world` (16) | yes |
| `lmcache` | `…-lmcache` | `--limit`, `--include…`, `--count`, `--agents-per-world` (16) | no |
| `swe_splice` (`swe-splice`) | `…-swe-splice` | `--limit`, `--include…`, `--count` (40), seed `--corpus-seed` | yes |
| `cipher` | `…-cipher` | `--limit`, `--include…`, `--count` (24), seed `--corpus-seed` | yes |
| `ai-village` | `…-ai-village` | `--mode window\|claude-code` (window), `--from` (2026-07-13), `--to` (2026-07-17), `--hours`, `--limit` | no |
| `demo-swarm` | `…-demo-swarm` | `--inputs`, `--truth`, `--run-lead-ms`, `--run-slack-ms`, `--seed` (holdout only) | no |

## Data and control flow

```text
export
  --version ≠ converter VERSION ─▶ refused
  --split/--release: dev needs none, holdout needs <detector>@<tag> (Release)
  demo-swarm ─▶ Inputs {root: deepest dir holding inputs and truth, truth, messages, exchanges, manifest}
             ─▶ demo_swarm::write_export(…, bench converter): manifest = the capture's manifest.json
                (source, converter, selection, revision = run id) + labels; bench provenance in diagnostics.json
             holdout (--split holdout --release <detector>@<version>):
               --out outside any git repository, else refused (before anything is read)
               truth header ─▶ run id, seed
               seed: bench.env in --inputs, else its parent (seed=N, else --seed N in swarm=), else --seed
                     both given and different ─▶ refused; ≠ truth header's seed ─▶ refused;
                     < 1,000,000 ─▶ refused (not a holdout run); nothing written
               demo_swarm::write_holdout_export: capture must be a dev view; same input-view check as dev;
                 then split = holdout, selection.release = the release,
                      selection.capture_digest = the capture manifest's digest (hex)
               commitment(manifest) ─▶ entry "<run id> <seed> <hex>" in splits/demo-swarm@1.holdout.commit
                 (a list sorted by run id): absent ─▶ added; same ─▶ matches; other seed or commitment ─▶ refused
  holdout ─▶ --out outside any git repository (enclosing_repository), else refused
  datasets.toml (--config, else the repo's, else DEFAULT) ─▶ data root (--root overrides)
  dataset dir = --dataset-dir, else root / datasets.<id>.path; source.path = that path as configured
  source_revision(dir, data root): HF snapshots/<hash> of the resolved path
                                   │ HF local dir: first line (commit) of every .cache/huggingface/download/**/*.metadata
                                   │   one commit ─▶ it; several ─▶ "mixed" + warn; lines not a 40-hex commit skipped
                                   │ git HEAD of the enclosing work tree, unless it is at or above the data root
                                   │ "unknown"
  check_pin(datasets.<id>.revision, actual, --allow-revision): differs ─▶ refused (or warn + record actual)
  Selection::dev(splits, id, n) (Unsplit without a list) | Selection::holdout(splits, id, n, release)
  datasets::export
    DatasetFlags::check (flags the dataset reads) ─▶ <crate>::Options ─▶ <crate>::source(dir, &options[, pace])
    corpus::export(source, out, ManifestInfo {version, source {path, revision, digest 0…},
                                              converter {CARGO_PKG_VERSION, A2A_BENCH_GIT},
                                              selection: options.settings(), pace}, selection)
    source.files_read().digest(dir) ─▶ manifest.source.digest ─▶ write_manifest (rewritten)
  holdout ─▶ commitment(manifest) ─▶ splits/<id>@<n>.holdout.commit: write, or equal else refused

validate
  read_manifest ─▶ open messages, exchanges, labels (if present), predictions (if given)
  headers' datasets = manifest's; predictions' manifest_digest = Manifest::digest
    (or, for a demo-swarm holdout export, its checked capture digest)
  per exchanges world: next world of each other file, same key
    WorldInputs::new ─▶ check_labels ─▶ check_predictions; counts by row kind
  trailers (counts, digest) ─▶ = manifest.files; manifest worlds (keys, order, exchanges, label rows)
  ─▶ ValidateReport {counts, findings (file, world, problem: ids only)}

run
  read_manifest ─▶ holdout? run_release (--holdout-release required, = manifest's release) + --out outside repos
  shlex::split(--detector-cmd) ─▶ input_view(export, out/input) (labels.jsonl absent, checked)
  <program> --input out/input --output out/predictions.jsonl [args…]   (stdout → our stderr; non-zero ─▶ error)
  --twice ─▶ again into out/predictions.second.jsonl; BLAKE3 differ ─▶ NotDeterministic
  score (below) into out/

score
  read_manifest ─▶ run_release; holdout: header version = tag (tagged_detector), --out outside repos, Disclosure::Holdout
  examples = 0 for swarm-traces
  demo_swarm::holdout::capture_digest(manifest): a demo-swarm holdout export's selection.capture_digest,
    which must equal the digest of its capture view (input view, split dev, release and
    capture_digest removed), else refused; None for every other export
  score_export(export, predictions, ScoreOptions {canonicalizer: ResourceCanon, example_cap, capture_digest})
    predictions' manifest_digest = the export's, or the capture digest
  GateSearch::from_env(--gates).load() ─▶ for_run(detector.name, variant).evaluate(score)
  Report::new ─▶ out/report.json, out/report.txt ─▶ table on stdout ─▶ exit_code (2 on a failed gate)

diff
  both dirs with manifest.json ─▶ manifests as JSON (source.digest removed) ─▶ differing JSON paths
                                ─▶ messages, exchanges, labels (when both have them)
  both files ─▶ predictions
  per file: headers ─▶ JSON paths; worlds matched by key in lockstep (unmatched held until their pair comes)
    predictions header paths under /detector (name, version, variant, config_digest) ─▶ Tally.header
      (listed as "header differences", never counted: they do not make the files differ)
    world rows compared; rows keyed (kind, id | exchange | agent), compared as JSON text
    --normalize-ids, per world:
      agents::canonicalize: each attributed agent ─▶ "~<smallest exchange id>" (exchanges sorted),
        renamed in attribution, unattributed, matches[].from/to, co_access[].from/to;
        an agent with no attribution row keeps its name
      predicted transmissions keyed by the digest of the row without its id
      row order within a world not compared
    only in a / only in b / changed / row order / world order
  ─▶ Tally {counts, header paths, first N (change, file, world, kind, id)}
```

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/cli/Cargo.toml` | package `a2a-bench-cli`, binary `a2a-bench` | |
| `crates/cli/build.rs` | the bench commit at build time (`git rev-parse HEAD`, else `unknown`), rebuilt when HEAD moves | env `A2A_BENCH_GIT` |
| `src/lib.rs` | crate root | modules |
| `src/args.rs` | the clap command line | `Cli`, `Command` |
| `src/repo.rs` | the checkout's paths and identity | `BENCH_GIT`, `CONVERTER_VERSION`, `root`, `datasets_config`, `splits_dir`, `converter` |
| `src/canon.rs` | the scorer's seam filled with `a2a_bench_resource::canonicalize` | `ResourceCanon` |
| `src/revision.rs` | a dataset dir's revision (HF snapshot, HF local dir, git, unknown), the pin | `Revision` (`HfSnapshot`, `HfLocalDir`, `Mixed`, `Git`, `Unknown`), `UNKNOWN`, `MIXED`, `HF_DOWNLOAD_CACHE`, `source_revision`, `check_pin`, `Pin`, `RevisionError` |
| `src/holdout/mod.rs` | commitment, commit file, release and repository rules | `COMMIT_CONTEXT`, `commitment`, `commit_path`, `record_or_check`, `Committed`, `outside_repository`, `run_release`, `tagged_detector`, `HoldoutError` |
| `src/holdout/demo_swarm.rs` | demo-swarm holdout runs: the seed (bench.env, `--seed`, truth) and the per-run commitment list | `MIN_SEED`, `BENCH_ENV`, `bench_env_seed`, `run_seed`, `RunSeed`, `SeedFrom`, `SeedError`, `Entry`, `record_or_check` |
| `src/safe.rs` | error text without dataset text | `read_error` |
| `src/datasets/mod.rs` | the datasets | `DatasetName` (`id`, `version`, `paced`) |
| `src/datasets/flags.rs` | dataset flags, applicability, pace, AI Village options | `DatasetFlags` (`check`, `pace`, `seed`, `ai_village`), `VillageMode`, `FlagError`, `DEFAULT_PACE_MIN_MS`, `DEFAULT_PACE_MAX_MS` |
| `src/datasets/dispatch.rs` | per-dataset `Options` and `source`, the export and source digest | `export`, `export_demo_swarm` (dev or holdout), `demo_swarm_inputs`, `ExportRequest`, `ExportSummary`, `DatasetError` |
| `src/commands/export.rs` | `export` | `ExportArgs`, `SplitArg`, `export`, `ExportOutcome` (`render`), `ExportCommandError` |
| `src/commands/validate.rs` | `validate` | `ValidateArgs`, `validate`, `ValidateReport` (`passed`, `render`), `Finding`, `ValidateError` |
| `src/commands/input_view.rs` | `input-view` | `InputViewArgs`, `input_view`, `render` |
| `src/commands/run.rs` | `run` | `RunArgs`, `run`, `RunOutcome`, `RunCommandError`, `INPUT_DIR`, `PREDICTIONS_FILE`, `SECOND_PREDICTIONS_FILE` |
| `src/commands/score.rs` | `score` | `ScoreArgs`, `ScoreRequest`, `score`, `detector_of`, `ScoreError`, `NO_EXAMPLES_DATASET` |
| `src/commands/diff/mod.rs` | `diff`: entry, manifests and headers, report | `DiffArgs`, `diff`, `Tally` (`render`, `equal`), `Difference`, `Change`, `DiffError`, `DEFAULT_FIRST` |
| `src/commands/diff/rows.rs` | one file's worlds and rows | (internal) `file`, `json_paths` |
| `src/commands/diff/agents.rs` | `--normalize-ids`: detector agents named by their smallest exchange | (internal) `canonicalize` |
| `src/bin/a2a-bench/main.rs` | parsing, printing, exit codes, logging (anyhow only here) | |
| `tests/common/mod.rs` | the binaries (`a2a-reference` found beside `a2a-bench`, built with `$CARGO` if missing), fixtures, scratch dirs inside and outside git | |
| `tests/e2e.rs` | export → validate → run → score → gates on SALT; reruns byte-identical; input view; tampering; diff; every fixture converter; flags; pins | |
| `tests/holdout.rs` | holdout refusals, commitment, release runs, aggregate reports | |
| `tests/demo_swarm_holdout.rs` | demo-swarm holdout runs over the crate's synthetic capture (`#[path]` to its fixture): manifest marks, seed rule, commit list, inside-repo refusal, dev unchanged, the capture's predictions and the reference detector scored with aggregates, image-digest releases | |
| `tests/canon.rs` | channel resources differing only in canonical form align | |
| `tests/units.rs` | revisions (HF symlink, HF local dir common and mixed, git guard, unknown), pins, commitment, flags, names |
| `tests/diff_agents.rs` | `diff --normalize-ids`: same split under other names diffs empty, another split differs, header-only differences reported apart | |

## Invariants and constraints

- **No dataset text on any output.** Commands print counts, ids, digests,
  paths and the report table; `validate` reduces JSON row errors to their
  line number (serde may quote a row); swarm-traces is always scored with
  `--examples 0`. Converter world failures are logged by `corpus::export`
  (their errors are the converters', which keep payloads out).
- **Exports are deterministic.** Same source, flags, split and bench commit:
  byte-identical directories (tested). The manifest's source digest covers
  exactly the files the converter read (`files_read`), relative to the
  dataset dir; `converter.git` is the build's commit.
- **Revisions.** HF snapshot hash (from the resolved path), else the
  commit an `hf download --local-dir` tree's metadata files share (`mixed`,
  with a warning, when they disagree), else the git HEAD of the enclosing
  work tree when it is below the data root, else `unknown`. A pinned revision that differs refuses the export without
  `--allow-revision`; the manifest always records the actual revision.
- **Holdout (design §9.2).** Only `--split holdout --release
  <detector>@<tag>` exports it; never inside a git repository; needs a dev
  list (`corpus::Selection::holdout`); the commitment (BLAKE3 derive-key
  `a2a-bench/1 holdout commit` over each world key + `\n`, `0x00`, the
  labels digest's hex) is written to `splits/<id>@<n>.holdout.commit` or
  must equal it. `run`/`score` of a holdout export need `--holdout-release`
  equal to the export's release, a detector whose header `version` is the
  tag, and an output outside any git repository, and write an aggregate
  report (`Disclosure::Holdout`). A dev export refuses `--holdout-release`.
- **demo-swarm holdouts are whole runs.** A run is a holdout run when its
  seed (from `bench.env`, else `--seed`; equal to the truth header's) is at
  least 1,000,000; anything else is refused before any file is written.
  The export is the capture's manifest plus labels, with exactly three
  marks: `split: holdout`, `selection.release` and
  `selection.capture_digest` (the capture manifest's `Manifest::digest`).
  Its input view therefore has its own digest, which a detector run on it
  (`run`) names; the adapter's predictions name the capture's, which
  `score` and `validate` accept only for a demo-swarm holdout export and
  only after rebuilding the capture view from the export (marks undone)
  and finding the recorded digest equal. Any other digest is refused. The
  commitment list `splits/demo-swarm@1.holdout.commit` holds one line per
  run, `<run id> <seed> <commitment hex>`, sorted by run id, the
  commitment being the usual one over the run's one world key and labels
  digest; a listed run must come back with the same seed and commitment.
  `--seed` on a dev export is refused; dev exports are otherwise as before.
- **Releases.** `<detector>@<version>` splits at the first `@`; the
  version may hold `:` (crosstalk's gateway export is versioned by image
  digest, `crosstalk-gateway-export@sha256:…`), not `@`, `/` or `\`.
- **`diff --normalize-ids` names agents by exchange sets.** Exchange sets
  are disjoint within a world, so `~<smallest exchange>` is unique; two
  detectors that split exchanges into agents identically diff empty
  whatever their agent and transmission ids. Detector header fields are
  reported, never counted.
- **The input view never holds `labels.jsonl`** (built by
  `corpus::input_view`, checked again by `run`).
- **The detector contract.** `<program> --input <dir> --output <file>`
  then the detector's own args; a non-zero exit fails the run; `--twice`
  requires byte-identical predictions.
- **Scoring uses the bench canonicaliser** on labels and predictions; the
  score crate stays free of `a2a-bench-resource`.
- **Gates** come from `--gates`, `A2A_BENCH_GATES`, the repository's
  `gates/`, selected by the predictions header's detector name and variant.
- No `unsafe`; no unwrap/expect/panic outside tests; `thiserror` errors in
  the library, `anyhow` only in the binary.

## Real-data smoke test (2026-10-06)

Release build, `~/Data/ai/agents` via the repository's `datasets.toml` (no
pins), outputs under `target/smoke/`. Counts only.

| Selection | Worlds | Exchanges | Label rows | Transmissions / controls | Revision | Export time | Validate |
| --- | ---: | ---: | ---: | --- | --- | ---: | --- |
| `salt --limit 8` | 8 | 1,184 | 2,827 | 200 / 1,443 | unknown (now HF local dir `2eba8f37…`) | 3.3 s | valid |
| `agentdojo --limit 10` | 10 | 48 | 72 | 8 / 16 | git `089ed46` | 10.4 s | valid |
| `collusion-wiki --demo` | 5 | 156 | 353 | 134 / 63 | unknown | 0.8 s | valid |

The SALT and wiki counts equal the converters' ct-eval comparisons
(dataset-salt.md `--limit 8`; dataset-wiki.md `--demo`: 5 worlds, 33
agents, 156 exchanges, 197 labels). No world failed. A second SALT export
diffs empty (`diff`: 24 file-worlds, 7,103 rows, 0 differences).

`run` with `a2a-reference` (`--twice`: both predictions files identical):

| Selection | Predictions | Overall recall | Precision | Other | Gates (`gates/reference.toml`) |
| --- | ---: | --- | --- | --- | --- |
| `salt --limit 8` | 1,554 | 0.875 (140 / 160) | 0.701 (467 correct, 199 false) | forwarding 40 / 40; 168.1 FP per 1k exchanges | 3 pass, 2 fail (verbatim 0.835 < 0.94, construction 0.875 < 0.88): exit 2 |
| `agentdojo --limit 10` | 11 | 1.000 (2 / 2) | 0.250 (2 correct, 6 false) | | 1 pass, 1 fail (precision 0.25 < 0.70): exit 2 |
| `collusion-wiki --demo` | 370 | 1.000 (134 / 134) | 1.000 (181 correct, 189 unjudged) | | none for the wiki: exit 0 |

The reference gates were calibrated on SALT `--limit 265` and full
AgentDojo selections, so small selections are expected to miss them.
Scoring the SALT predictions again with `score` gives a byte-identical
`report.json`, and `validate --predictions` passes on each run.

## demo-swarm parity on node0's saved runs (2026-10-06, P7)

Captures and predictions from `ct-bench-detect from-export`, truth from
the runs, `score --gates gates/crosstalk-gateway-export.toml --examples
0`. The export's input view is the capture's manifest, so the predictions'
`manifest_digest` matches and they score; `validate --predictions` passes.

| Run | Recall | Precision | FP / 1k exchanges | Gates |
| --- | --- | --- | --- | --- |
| `20261006T020835Z` (headline) | 1.000 (55 / 55) | 1.000 (55 correct, 0 false) | 0.0 | 5 pass, exit 0 |
| `20261006T021639Z` (boilerplate) | 1.000 (50 / 50) | 0.883 (166 correct, 22 false) | 89.1 | 3 pass, exit 0 |
