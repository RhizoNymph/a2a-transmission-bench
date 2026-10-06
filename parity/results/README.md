# Parity results: P2–P4 against ct-eval

Stages from [../../docs/design/separation.md](../../docs/design/separation.md) §7.2,
run on 2026-10-06 at bench commit `2a24ddd` (`feat/cli`), release build.

- **Golden inputs:** crosstalk `9707b1d` (PR #108): `ct-eval export --format a2a-bench/1`
  and `ct-eval run --predictions-out`, in
  `~/Code/ai/crosstalk/golden-scratch/sanity/<run>/`. Predictions are ct-eval's reference
  matcher, except headline/boilerplate (crosstalk gateway export).
- **Baseline:** ct-eval's own `report.json` at crosstalk `7f8a2fb`
  (`parity/baseline-7f8a2fb/`, kept on the design branch's worktree).
- **Datasets:** `~/Data/ai/agents` through the repository's `datasets.toml`.
- Every result file here holds counts, ids, digests, gate names and selection settings
  only; no message, label or source text.

Tools (`parity/tools/`, Python 3, streaming one world at a time):

| Tool | Compares |
| --- | --- |
| `compare_reports.py` | two `report.json` (ct-eval's or the bench's), normalising the shape differences in `docs/features/score.md`: `detector` string vs object, `quality {type, data}` vs `{kind, …}`, `failures` vs `failed_worlds`; `background.sources` by (reason, count) only |
| `compare_predictions.py` | two predictions files with detector agents replaced by the exchange set of their `attribution` row and transmission ids dropped |
| `compare_labels.py` | two exports' `labels.jsonl` with label ids dropped; unmatched rows paired and counted by differing field name |
| `merge_p3.py`, `merge_p4.py` | write the per-run summaries in `p3/`, `p4/` |

## P2: scorer

`a2a-bench score --export <golden>/export --predictions <golden>/predictions.jsonl
--examples 0 --gates gates/reference.toml` (headline/boilerplate: `--gates gates/`, which
selects `crosstalk-gateway-export.toml`), compared with the baseline report with
`compare_reports.py`: totals, overall, out_of_reach, forwarding, access_only, every row
(dataset × route × carrier × class × tier, all counts and ratios), transmissions by
(dataset, route, quality), violations by reason, access-only under controls, gates (status
and value, by name), failed worlds, unscored, unknown detected agents, background
(false positives, exchanges, FP/1k, sources' reasons and counts), misses and false
positives (both empty). Ratios are compared exactly (floating point equal).

| Run | Baseline | Result | Values compared | Expected | Found | Predicted | Correct | FP | Recall | Precision | Forwarding | Out of reach | FP/1k | Violations | Gates |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| salt53 | salt-53-reference | **equal** | 85 | 2909 | 2824 | 10978 | 6398 | 4580 | 0.971 | 0.583 | 738/941 | 0/0 | 388.3 | rejected_send 160, boilerplate 89, shared_source 12 | 5 pass, 4 other dataset |
| agentdojo | agentdojo-reference | **equal** | 77 | 2324 | 2324 | 3912 | 3172 | 740 | 1.000 | 0.811 | – | 87/359 | 65.9 | boilerplate 45 | 2 pass, 7 other dataset |
| wiki-demo | wiki-demo-reference | **equal** | 73 | 134 | 134 | 370 | 181 | 0 (189 unjudged) | 1.000 | 1.000 | – | – | 0.0 | – | 9 other dataset |
| swarm-traces | swarm-traces-reference | **equal** | 71 | 629 | 627 | 1261 | 1261 | 0 | 0.997 | 1.000 | – | – | – | – | 9 other dataset |
| cipher50 | cipher-50-reference | **equal** | 75 | 200 | 86 | 86 | 86 | 0 | 0.430 | 1.000 | – | 0/200 | – | – | 9 other dataset |
| tau2 | tau2-reference | **equal** | 77 | 119256 | 119085 | 241794 | 241233 | 561 | 0.999 | 0.998 | – | – | 2.1 | shared_source 481 | 2 pass, 7 other dataset |
| headline | bench-20261006T020835Z-headline-swarm | **equal** | 69 | 55 | 55 | 55 | 55 | 0 | 1.000 | 1.000 | – | – | 0.0 | – | 5 pass, 3 other dataset |
| boilerplate | bench-20261006T021639Z-boilerplate-swarm | **equal** | 72 | 50 | 50 | 188 | 166 | 22 | 1.000 | 0.883 | – | – | 89.1 | – (access-only under controls: reread/discarded 2) | 3 pass, 5 other dataset |
| agentdojo-full | none | **no-baseline** | – | 45259 | 45259 | 69398 | 58625 | 10773 | 1.000 | 0.845 | – | 1625/5961 | 56.5 | boilerplate 878 | 2 pass, 7 other dataset |

No failed worlds, unscored worlds or unknown detected agents in any run. headline and
boilerplate equal the baseline's `swarm` reports exactly, so the `swarm_truth/detected.rs`
change between `7f8a2fb` and `9707b1d` has no effect on these two runs' counts.
agentdojo-full (all 36,679 worlds, no selection) has no ct-eval report; its counts are
recorded in `p2/agentdojo-full.json` and serve as the bench's own baseline.

## P3: reference detector

`a2a-bench input-view <golden>/export`, `a2a-reference --input … --output pred.jsonl`, then:
(1) `a2a-bench diff --normalize-ids <golden>/predictions.jsonl pred.jsonl`;
(2) `compare_predictions.py` on the same pair; (3) `a2a-bench score` of `pred.jsonl`
compared with P2's report (golden predictions) by `compare_reports.py`.

| Run | Result | Worlds (equal) | Attribution partitions differ | Transmissions golden / bench / matched | `diff --normalize-ids` | Score vs P2 |
| --- | --- | --- | ---: | --- | --- | --- |
| salt53 | **equal** | 53 (53) | 0 | 4244 / 4244 / 4244 | 4335 rows each; all rows only-in-a/only-in-b, 3 header paths | equal (85 values) |
| agentdojo | **equal** | 2259 (2259) | 0 | 2335 / 2335 / 2335 | 6481 rows each; same | equal (77) |
| wiki-demo | **equal** | 5 (5) | 0 | 100 / 100 / 100 | 133 rows each; same | equal (73) |
| swarm-traces | **equal** | 629 (629) | 0 | 627 / 627 / 627 | 1885 rows each; same | equal (71) |
| cipher50 | **equal** | 400 (400) | 0 | 86 / 86 / 86 | 886 rows each; same | equal (75) |
| tau2 | **equal** | 10832 (10832) | 0 | 113569 / 113569 / 113569 | 131585 rows each; same | equal (77) |
| agentdojo-full | **equal** | 36679 (36679) | 0 | 43141 / 43141 / 43141 | 112707 rows each; same | equal (79) |

The `a2a-bench diff --normalize-ids` output is not empty, and the reason is the designed
id difference: it drops transmission ids but still keys `attribution` rows by agent id and
compares the agent ids inside transmissions (`from`, `to`). ct-eval names a detector agent
by a ULID; `a2a-reference` names it `k:<credential digest>`. Every attribution and
transmission row therefore shows as only-in-a plus only-in-b, and the 3 changed paths are
the header's `detector.version` (`9707b1d…` vs `0.1.0`), `detector.variant` (`default` vs
`max-postings-50`) and `detector.config_digest` (bench only). With agents named by the
exchange set they attribute (`compare_predictions.py`), every world is equal: the same
partition of exchanges into agents (one credential per true agent on all seven runs), the
same world statuses, and the same multiset of transmissions (state, quality, every match
with its locations, carrier, route). Row order within a world was not compared.

## P4: converters

`a2a-bench export` with ct-eval's selection, then `a2a-bench diff <golden>/export <bench
export>`; label rows also compared without ids (`compare_labels.py`) where the ids differ
by design.

| Run | Flags | Result | File-worlds / rows | messages, exchanges | labels | Manifest paths changed (besides ignored) |
| --- | --- | --- | --- | --- | --- | --- |
| salt53 | `--dataset salt --limit 53` | **byte-identical files** | 159 / 71378 | equal digests | equal digest (ids `t<n>` equal) | `selection.split_list` |
| agentdojo | `--dataset agentdojo --include pipeline=gpt-4o-2024-05-13 --include pipeline=claude-3-5-sonnet-20241022 --include pipeline=gemini-1.5-pro-002 --include attack=important_instructions --include attack=none` | **byte-identical files** | 6777 / 54401 | equal | equal | `selection.split_list` |
| agentdojo-full | `--dataset agentdojo` | **byte-identical files** | 110037 / 958812 | equal | equal | `selection.split_list` |
| tau2 | `--dataset tau2` | **byte-identical files** | 32496 / 1321530 | equal | equal | `selection.split_list` |
| wiki-demo | `--dataset collusion-wiki --demo` | **equal modulo label ids** | 15 / 740 | equal | 197 ids differ; 353 rows equal without ids | `files.labels`, `selection.{family,limit,max_agents,min_agents,split_list}` |
| swarm-traces | `--dataset swarm-traces` | **equal modulo label ids** | 1887 / 8177 | equal | 629 ids differ; 2516 rows equal without ids | `files.labels`, `selection.split_list` |
| cipher50 | `--dataset cipher --count 50` | **equal modulo label ids** | 1200 / 4800 | equal | 400 ids differ; 1400 rows equal without ids | `files.labels`, `selection.split_list` |

Ignored as instructed: `converter.git` (bench commit vs crosstalk commit), `source.digest`
(the bench digests the files its converter reads; equal for salt and swarm-traces, differs
for the others), `source.revision`, `source.path` (equal everywhere). The manifest's
per-world entries (key, exchanges, labels, notes) are equal on every run.

Every remaining difference, explained:

- **`selection.split_list: "none"`** (all runs): the bench records that no split list
  selected the worlds (`corpus::Selection::dev` without a list); ct-eval has no splits.
  Settings encoding, by design.
- **wiki `--demo` selection**: ct-eval records `{"demo": true}`; the bench records the
  settings `--demo` expands to as well (`family: [relay-coordination]`, `limit: 5`,
  `max_agents: 12`, `min_agents: 2`). The worlds, messages and exchanges are identical, so
  the expansion is the same. Settings encoding, by design.
- **Label ids** (wiki, swarm-traces, cipher): the golden export numbers labels `t<n>`; the
  bench converters use their own ids, as their docs list (wiki: the source path,
  `/rev/<rev_id>/…`; swarm-traces: the source path `/row/<id>/token/<n>`; cipher:
  `cipher/<world key>`). With ids removed every label row (transmission, negative control,
  `exchange_agent`, world coverage) is equal, and `files.labels` differs only because of
  them. SALT, AgentDojo and τ² use the golden export's ids and their labels files are
  byte-identical.
- **`source.revision`** (ignored, noted for the record): salt golden
  `2eba8f3771e8fbcc0f49f6cbbfd2111b939a117a`, bench `unversioned`. `salt-nlp` is an
  `hf download --local-dir` copy (revision in `.cache/huggingface/download/*.metadata`), not
  an HF cache snapshot, and the bench's `source_revision` recognises only `snapshots/<hash>`
  paths and git work trees. wiki-demo and swarm-traces: golden `unknown`, bench
  `unversioned` (spelling of "no revision").

No signed-message id differences arose: every messages file is byte-identical.

## P5: crosstalk's adapter (ct-bench-detect) vs ct-eval

Run by crosstalk-rollouts at crosstalk 4d3d2c3 (PR #112 HEAD, stacked on
#108): `ct-bench-detect` on the bench input view and `ct-eval run
--detector live --predictions-out` on the same selection write
**byte-identical** predictions files (header and trailer included) on
SALT `--limit 53` forwarding off, SALT `--limit 53` forwarding on, and
wiki `--demo`. ct-bench-detect also checks every part's bench text against
crosstalk-spec's `part_text` and fails a world on any mismatch; none did,
which closes P1 directly.

## P6: end to end (bench scorer on ct-bench-detect predictions vs ct-eval's live report, same commit)

Detection has changed since the 7f8a2fb baseline, so P6 compares like
with like: ct-eval live reports and ct-bench-detect predictions both at
crosstalk 4d3d2c3, scored by `a2a-bench score --gates
gates/crosstalk-live.toml --examples 0` on the 4d3d2c3 golden exports.

| Run | Result | Recall / precision | Gates |
| --- | --- | --- | --- |
| wiki `--demo`, forwarding off | equal (84 values) | 0.985 / 1.000 | none apply |
| SALT `--limit 53`, forwarding off | equal (93 values) | 0.854 / 0.955 | 5/5 pass |
| SALT `--limit 53`, forwarding on | equal (82 values) | 0.894 / 0.882 | forwarding gate fails on both sides (0.885 < 0.94; the 0.86 bound is on `chore/gates-salt-forwarding-on`) |

The only listed differences are the lengths of the `misses` and
`false_positives` example lists, because ct-eval's reports were written with
its default 50 examples and the bench with `--examples 0`. They are not
counts. Per-run outputs: `p6/`.

## P7: node0 bench runs

`ct-bench-detect from-export` (crosstalk 4d3d2c3) turns each saved node0
run into a bench capture and gateway-export predictions. `a2a-bench export
--dataset demo-swarm` labels the capture from the run's truth v2, keeping
the capture's manifest (`fix/cli-parity-followups` d6673b1), and `a2a-bench
score --gates gates/crosstalk-gateway-export.toml --examples 0` scores it:

| Run | Recall | Precision | FP/1k | Gates | ct-eval (7f8a2fb baseline) |
| --- | --- | --- | --- | --- | --- |
| 20261006T020835Z (headline) | 1.000 (55/55) | 1.000 | 0.0 | 5/5 pass | 1.000 / 1.000, 5/5 |
| 20261006T021639Z (boilerplate) | 1.000 (50/50) | 0.883 (166 correct, 22 false) | 89.1 | 3/3 pass | 1.000 / 0.883, 89.1, 3/3 |

Equal. These saved runs predate the gateway's placement reads, so
attribution comes from the saved evidence and no `origin_at` is known; that
is ct-eval's export path too. Future runs fetch both
(`ct-bench-detect fetch`).

## P1: part text

Closed directly by P5 (ct-bench-detect checks bench part text against crosstalk-spec per part; no world failed) and indirectly by P4 below.

### P4's indirect argument

P4's `messages.jsonl` digests are equal on all seven converter runs (1,061,585 messages,
including τ² full and AgentDojo full), so the bench's canonical message JSON, message ids
and every part's text equal the golden export's byte for byte. The golden exporter
checked every label's text against `text_at` on the spec side, and the labels are equal
(byte-identical, or equal without ids), so every label's text resolves to the same bytes in
the bench's messages. What P4 cannot show is that the golden export's part text equals the spec's own part
text; the golden exporter is a serialisation of the spec types, and the per-part check
against the spec is P5 (crosstalk's adapter).

## Timings (wall, this machine, load ~10–15)

| Run | P2 score | P3 input-view / a2a-reference / diff / score | P4 export / diff | ct-eval golden export / run (summary.txt) |
| --- | ---: | --- | --- | --- |
| salt53 | 3.4 s | 0.08 / 6.5 / 0.3 / 1.6 s | 13.4 / 5.3 s | 216 / 187 s |
| agentdojo | 0.9 s | 0.02 / 1.9 / 0.1 / 0.5 s | 1.8 / 1.4 s | 82 / 16 s |
| agentdojo-full | 13.3 s | 2.6 / 58.3 / 3.6 / 12.3 s | 113 / 43 s | 135 / 136 s |
| tau2 | 20.9 s | 1.5 / 58.6 / 10.1 / 31.9 s | 29.5 / 72.2 s | 409 / 475 s |
| wiki-demo | 0.03 s | 0.01 / 0.03 / 0.02 / 0.02 s | 0.24 / 0.03 s | 1.3 / 0.9 s |
| swarm-traces | 0.08 s | 0.01 / 0.07 / 0.04 / 0.06 s | 3.1 / 0.4 s | 5.8 / 5.2 s |
| cipher50 | 0.04 s | 0.01 / 0.04 / 0.01 / 0.03 s | 0.05 / 0.10 s | 0.3 / 0.2 s |
| headline, boilerplate | < 0.1 s | – | – | – |

Peak RSS: tau2 export 159 MB, agentdojo-full export 58 MB, a2a-reference on tau2 14 MB,
P2 score on tau2 18 MB.
