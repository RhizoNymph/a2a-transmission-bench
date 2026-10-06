# Scoring (`a2a-bench-score`)

The scorer, the report and the gates: a detector's `predictions.jsonl`
against an export's `labels.jsonl`, with the export's messages and
exchanges to resolve locations. Ported from crosstalk-eval's `score/`,
`report/` and the bench-side half of `predict/` at crosstalk `7f8a2fb`; the
rules mean what they meant there (crosstalk `docs/features/eval.md`,
"Invariants and constraints" and "Gates file"). Design:
[../design/separation.md](../design/separation.md) §3.6, §5.4, §6.2, §7, §9.2.

## Scope

- Turning prediction rows into the scorer's predictions: attribution to
  true agents, unattributed agents, one prediction per content match or
  co-access (`predict`).
- The alignment rule and the judge (`score::align`, `score::judge`).
- The scorer: counts per dataset × route kind × carrier × class × tier,
  transmission rows keyed by `quality`, violations by reason, the sources
  tally, access-only predictions under controls, examples (`score`).
- `report.json` and the text table, in full or holdout disclosure
  (`report`).
- Gates: the `gates/*.toml` files, their search, selection by detector name
  and variant, evaluation, the exit code (`report::gates`).
- A library entry point that streams an export and a predictions file
  world by world and returns the run's summary (`run`).

## Non-scope

- The `a2a-bench` CLI (its `score` command calls `run::score_export`, then
  `Report::new`, `Report::write`, `Report::exit_code`).
- Resource canonicalisation: `a2a-bench-resource` plugs in through
  `canon::Canonicalize`; the crate itself compares resources as given
  (`AsGiven`).
- crosstalk's `DetectionQuality` cross-check (a spec type): it moves to the
  crosstalk adapter's tests. The bench keeps the transmission rows it
  checked.
- Identity and channel-discovery scoring: `agent_cluster` labels are read
  and ignored.

## Data and control flow

```text
run::score_export(export dir, predictions path, ScoreOptions)
  read manifest.json ──▶ Manifest::digest ──▶ options.manifest_digest
                     ──▶ manifest.files   ──▶ options.file_digests
  run::score_streams(Streams { messages, exchanges, labels, predictions })
    FileReader::open × 4 (headers: file kind, format) ─ datasets equal, predictions' manifest_digest equal
    loop over exchanges.jsonl's worlds (the export's order):
      next_world() on the other three ─ same key, else WorldOrder / MissingWorld
      WorldInputs::new(messages, decl, exchanges)    ─ error fails the run (export broken)
      check_labels(inputs, labels)                    ─ error fails the run
      predictions world status:
        no_consumers {ingested} ─▶ Unscored { worlds += 1, ingested += n }
        failed {reason}         ─▶ FailedWorld { Detector }
        scored:
          check_predictions(inputs, rows)              ─ error ─▶ FailedWorld { InvalidPredictions }
          predict::world_predictions(labels, rows)
            exchange_agents(labels) ─▶ AgentMap::from_rows ─ merge ─▶ FailedWorld { MergedAgents }
            per transmission row: from_transmission ─ unknown agent ─▶ UnknownDetected (not scored)
          Scorer::add_world(World { dataset, inputs, labels, coverage }, predictions)
            Judge::new(world, canonicaliser)
            per prediction: Judge::judge ─▶ Outcome, row counts, transmission verdicts,
                            violations / access_only_under_controls / sources / examples
            per positive label: expected, found | missed (+ suspected)
    every file read to its trailer; extra worlds ─▶ ExtraWorld
    with options.file_digests: FileReader::trailer().digest of messages and
      exchanges (and labels when the manifest has its digest) ─ differs ─▶ FileDigest
  ─▶ RunSummary { dataset, detector, score, failures, unscored, unknown_detected_agents }

Gates: GateSearch::from_env(--gates).load() ─▶ Gates::for_run(detector.name, detector.variant)
       ─▶ Gates::evaluate(&summary.score) ─▶ Vec<GateOutcome>
Report::new(summary, outcomes, Disclosure) ─▶ to_json (report.json), table::render (report.txt),
                                              exit_code (0, or 2 on a failed gate)
```

### Predictions from rows (`predict`)

- `AgentMap::from_rows`: each detector agent of an `attribution` row maps
  to the true agent (the `exchange_agent` rows) of its exchanges. Several
  detector agents for one true agent (a split) is fine; one detector agent
  over two true agents fails the world with `AgentMapError::Merged
  { agent, first, second }` (first in row and exchange order).
- `unattributed` agents, and attributed agents holding no exchange, have no
  true agent: a transmission naming one is reported as an
  `UnknownDetectedAgent` (ct-eval's `unknown_detected_agent` diagnostic)
  and none of its predictions are scored.
- `from_transmission`: confirmed, classified or aggregated make one
  prediction per content match, of the match's class, carrier and route,
  at its `read_at`, with its `origin_at`. Suspected and discarded make one
  per co-access: writer to reader, at the read's exchange and `read_at`,
  carrier `tool_result`, route `channel {resources: [resource]}`, origin the write's
  `write_at`, class `suspected` or `discarded`. Detected and
  awaiting-content make none. Every prediction carries the transmission's
  `quality`.

### Judging (`score::judge::Judge::judge`)

1. The first positive label (content or access-only) it aligns with:
   `Correct` at that label's tier. The scorer marks every aligned label:
   content evidence finds a content label, access evidence finds an
   access-only label and marks a content label `suspected`; content
   evidence on an access-only label finds nothing.
2. Else a `discarded` prediction is `Dismissed` (with the most specific
   control it falls under, recorded, never charged).
3. Else an exemption covering it (same reader and reader exchange,
   overlapping location; sender not compared): `Unjudged`.
4. Else the most specific negative control it falls under (location, then
   origin, then exchange only): `False { violated, tier }`.
5. Else coverage: `Complete { tier }` is `False { violated: None, tier }`,
   `Partial` is `Unjudged`.

Alignment (`score::align::aligns`): same sender and reader, same reader
exchange, overlapping reader locations (same message and part, a shared
byte), and for a channel label a channel prediction (`PredictedRoute::Channel
{resources}`) one of whose resources equals the label's resource, each
compared after the canonicaliser. An empty resource list aligns with no
channel label.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/score/src/lib.rs` | crate root | modules, `AsGiven`, `Canonicalize`, `World` |
| `src/canon.rs` | the canonicalisation seam | `Canonicalize`, `AsGiven`, `same_resource` |
| `src/class.rs` | evidence classes and a need's class | `EvidenceClass`, `need_class` |
| `src/world.rs` | one world as the scorer sees it | `World` |
| `src/predict/mod.rs` | prediction rows to predictions | `Prediction`, `from_transmission`, `world_predictions`, `WorldPredictions`, `UnknownDetectedAgent` |
| `src/predict/agents.rs` | attribution | `AgentMap`, `AgentMapError`, `exchange_agents` |
| `src/score/mod.rs` | the score and row selection | `Score`, `Selector`, re-exports |
| `src/score/align.rs` | the alignment rule, controls, exemptions | `aligns`, `violates`, `specificity`, `exempts` |
| `src/score/judge.rs` | judging one prediction | `Judge`, `Outcome`, `Expects`, `Positive` |
| `src/score/rows.rs` | row keys and counts | `RowKey`, `Counts`, `TransmissionKey`, `TransmissionCounts`, `Row`, `TransmissionRow`, `ViolationRow`, `AccessOnlyControlRow`, `Miss`, `FalsePositive`, `Totals`, `ratio`, `EXCERPT_CHARS` |
| `src/score/scorer.rs` | accumulation per world | `Scorer` |
| `src/score/sources.rs` | violation sources tally | `SourceTally`, `SourceCount`, `source_key`, `TOP_SOURCES`, `SOURCE_CHARS` |
| `src/report/mod.rs` | `report.json` | `Report`, `Summary`, `ReportRow`, `AccessOnly`, `Background`, `Detail`, `Disclosure`, `ReportError`, `GATE_FAILURE_EXIT` |
| `src/report/table.rs` | the text table | `render` |
| `src/report/gates/mod.rs` | gates and their evaluation | `Gates`, `Gate`, `Check`, `GateOutcome`, `GateStatus` |
| `src/report/gates/parse.rs` | strict gates-file reading | `GateError`, `InvalidGate` |
| `src/report/gates/search.rs` | where gates come from | `GateSearch`, `GatesFrom`, `GatesLocation`, `GATES_ENV`, `REPO_GATES` |
| `src/run/mod.rs` | the streaming entry point | `score_streams`, `Streams`, `ScoreOptions`, `RunSummary`, `FailedWorld`, `WorldFailure`, `Unscored`, `UnknownDetected`, `DEFAULT_EXAMPLES` |
| `src/run/export.rs` | an export directory | `score_export` (manifest digest and file trailer digests checked) |
| `src/run/error.rs` | run errors | `RunError` (incl. `FileDigest`, `MissingTrailer`), `FileName` |
| `gates/reference.toml` | ct-eval's detector-less gates (9) | |
| `gates/crosstalk-live.toml` | ct-eval's `detector = "live"` gates (20; variants `forwarding-off`, `forwarding-on`) | |
| `gates/crosstalk-gateway-export.toml` | ct-eval's `gateway-export` gates (8) | |
| `crates/score/tests/common/mod.rs` | synthetic world builder (`Draft`), message and label helpers, file writers | |
| `tests/score.rs`, `score_many_labels.rs`, `score_discarded.rs`, `access_only.rs`, `tiers.rs` | ct-eval's scoring tests on bench types; forwarding, out-of-reach, exemptions, coverage | |
| `tests/predict.rs` | rows to predictions, splits, merges, unattributed agents | |
| `tests/gates.rs`, `tests/gates_search.rs` | gate parsing, selection, evaluation, shipped files, search | |
| `tests/report.rs`, `tests/run.rs` | report fields, holdout, determinism; the entry point | |

## Invariants and constraints

- **Rules unchanged from ct-eval.** One prediction may find several labels;
  only content finds a content label; access-only labels are found only by
  suspected or discarded predictions; a discarded prediction aligned with
  nothing is dismissed; only content violates a control (an access-class
  prediction under a control goes to `access_only_under_controls`);
  exemptions, then controls (most specific first), then coverage.
- **Apart from overall.** `overall` sums content rows whose tier is neither
  `out_of_reach` nor `forwarding`; those two are `out_of_reach` and
  `forwarding`. Access-only recall counts in-reach rows only
  (`Tier::in_overall`).
- **Selectors read content rows** unless they name `suspected` or
  `discarded`.
- **Transmission verdicts**: genuine when any prediction is correct, false
  detection when none is and one is false, unlabeled otherwise; keyed by
  dataset, the first prediction's route kind, and `quality`.
- **Failures apart.** A failed world (detector `failed`, invalid
  predictions, merged attribution) and an unscored world (`no_consumers`)
  are never scored as zero; their labels are not counted. A broken export
  (inputs or labels failing their checks, world order, framing,
  truncation, a foreign manifest digest, an export file whose trailer
  digest is not the one `manifest.files` records) fails the whole run.
- **Manifest notes are not reported.** The manifest's per-world `labels`
  counts and converter `notes` are not read: the report's shape is
  ct-eval's, and adding them is a report change, not a format one.
- **Determinism.** Every list in the score and report comes from ordered
  maps (`BTreeMap` by derived `Ord`); predictions are judged in row order.
  Equal inputs give byte-identical `report.json` (tested).
- **Holdout disclosure** writes the totals, summaries, rows, transmission,
  violation and access-only-under-control rows, gate outcomes and counts of
  failed worlds and unknown-agent transmissions; it leaves out `failures`,
  `unknown_detected_agents`, `misses`, `false_positives` and
  `background.sources`, and the table names no world.
- **Report shape.** ct-eval's field names and nesting are kept (rows
  flatten key and counts) so the parity check compares counts row by row;
  differences: `detector` is the predictions header's `DetectorInfo`,
  `failures` are typed `{world, failure: {kind, …}}`, plus `disclosure`,
  `failed_worlds`, `unknown_detected_agent` (and the list
  `unknown_detected_agents`); transmission rows are keyed by the bench's
  `Quality` instead of the spec's `QualityMatch`.
- **Gates.** Each file names one `detector`; a gate's optional `variant`
  selects the run's `detector.variant` (unset: every variant). Unknown keys
  and bounds that do not fit the metric are refused. `recall`/`precision`
  take `min`, `violations` a whole `max` (optional `reason`), `fp_per_1k`
  a `max`. A gate on a dataset the run did not score is `other_dataset`; a
  gate with no data is `skipped`. Thresholds are ct-eval's at `7f8a2fb`,
  verbatim. Search: `--gates`, `A2A_BENCH_GATES` (empty is unset), the
  repository's `gates/`; a path is a file or a directory of `*.toml` files
  read in name order; only a missing `--gates` is an error. A failed gate
  exits 2.
- **No crosstalk dependency.** The crate depends on `a2a-bench-format`,
  serde, serde_json, thiserror and toml only.
