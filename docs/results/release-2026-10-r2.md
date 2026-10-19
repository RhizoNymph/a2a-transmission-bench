# Release results, 2026-10, second held-out release

crosstalk against the bench's naive baseline `a2a-reference` on eight fresh
demo-swarm runs, made for this release under the bench's holdout protocol and
never scored or inspected on crosstalk's side. This is the held-out measurement
of crosstalk at staging `a74f808`, which adds order-tolerant turn threading and
retires crosstalk's own scorer since the first release
([release-2026-10.md](release-2026-10.md)). The public-dataset (dev) table is
in the first release and is not rerun here.

## Held-out results (fresh demo-swarm runs)

Eight runs of crosstalk's demo swarm through crosstalk's gateway on node0, made
for this release under the bench's holdout protocol (design §9): seeds
1000007–1000014 (unused by every earlier run, dev or holdout), four `headline`
and four `boilerplate` scenarios interleaved; saved without scoring (no report,
no metrics on the crosstalk side), stored outside every repository, and scored
once by the bench owner. Per-run outputs stay outside the repository; this
document reports aggregates only (design §9.2). Each run's commitment (run id,
seed, BLAKE3 over its world key and labels digest) is in
`splits/demo-swarm@2.holdout.commit`.

- **crosstalk:** staging `a74f8085d0476ee9fe3d3c7d229df60fc8cd5556`, postgres
  mode (the stock `deploy/demo/crosstalk.demo.json`), a fresh database per run.
  Gateway images (the predictions' `detector.version`):
  `sha256:638dbb5b8967a8d140ec63d51c29606cf4d87ca194b72c8535619c16ee327b05`
  (six runs) and
  `sha256:3ed9c6e71e7958c2e69fb2fa9230cd34557fac34c9f1fdcc7620a3f25027eb1d`
  (two runs): one build of a clean `a74f808` tree. The two
  ids have identical layers and differ only in the `com.docker.compose.service`
  label (`crosstalk` and `migrate`, which compose builds from the same
  Dockerfile under one tag; each run recorded whichever was tagged last), so
  every run ran the same gateway binary. Predictions are the gateway's own exported
  transmissions (`crosstalk-gateway-export`, variant `default`), with
  attribution and origins from the gateway's conversation reads, turned into
  bench files by `ct-bench-detect from-export`.
- **Bench:** tag `a2a-bench-v1.1.1` (`114217d459d43b7a77f876963a9681f313b78f2c`),
  dataset `demo-swarm@2` (truth joined by tool-use id), gates from that tag's
  `gates/crosstalk-gateway-export.toml`.
- **Reference:** `a2a-reference` `0.1.0` (same tag), run by the bench on each
  run's input view.
- **Date:** 2026-10-19 (UTC), runs `20261019T062225Z` to `20261019T070156Z`.

Recall is over the runs' labelled wiki transmissions; precision over the
predictions the labels judge; FP/1k is false positives per 1,000 exchanges.
Counts are summed over the four runs of each scenario.

| scenario (4 runs each) | detector | recall | precision | FP/1k | reread controls violated | runs passing every gate |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| headline | crosstalk | 1.000 (214/214) | 1.000 (214 correct, 0 false) | 0.0 | 0 | 4/4 |
| headline | a2a-reference | 0.977 (209/214) | 0.981 (209 correct, 4 false) | 4.1 | 0 | no gates |
| boilerplate | crosstalk | 1.000 (195/195) | 0.887 (765 correct, 97 false) | 97.2 | 0 | 3/4 |
| boilerplate | a2a-reference | 1.000 (195/195) | 0.044 (948 correct, 20,608 false) | 20,649.3 | 0 | no gates |

- `headline` traffic writes wiki pages with distinctive text; `boilerplate`
  traffic fills pages with shared template sentences, the hard case for content
  matching.
- One `boilerplate` run failed two of crosstalk's gates: false positives
  (FP/1k ≤ 115) and precision (≥ 0.86). Its recall gate passed, and the other
  three boilerplate runs passed every gate. Summed over the four runs the
  scenario is at 97.2 FP/1k and 0.887 precision, inside both bounds. Reported as
  measured; the gates were set from dev runs before these were made.
- Against the first release (crosstalk `7d678e2`, memory mode, six runs): headline is
  unchanged at 1.000 / 1.000 / 0.0; boilerplate moves from 0.890 precision and
  96.1 FP/1k to 0.887 and 97.2, with recall 1.000 in both. The two releases are
  different runs, so this compares two samples, not one build against another
  on the same traffic.
- The exchange counts the rates use: headline 982, boilerplate 998.
- Every run labelled all of its truth's transmissions (39 to 63 per run, none
  dropped).
- crosstalk's `detector.version` here is the gateway image digest (the export path
  has no adapter build); the crosstalk commit above is the one that built it.

## Reproduce

On node0, crosstalk at `a74f808`: `deploy/run.sh bench --holdout` with seeds
1000007–1000014 (swarm, watermark wait, `ct-bench-detect swarm-fetch`,
exchange-log and blobs snapshot, `ct-bench-detect fetch`, `ct-bench-detect
from-export --run <run> --out <run>/bench-input`; no scoring). Then, per run,
with the bench at `a2a-bench-v1.1.1` and output outside every git repository:

```text
V=$(head -1 <run>/bench-input/predictions.jsonl | jq -r .detector.version)

a2a-bench export --dataset demo-swarm --inputs <run>/bench-input --truth <run>/truth.jsonl \
  --out <out>/export-crosstalk --split holdout --release crosstalk-gateway-export@$V
a2a-bench score --export <out>/export-crosstalk --predictions <run>/bench-input/predictions.jsonl \
  --out <out>/crosstalk --gates gates/crosstalk-gateway-export.toml --examples 0 \
  --holdout-release crosstalk-gateway-export@$V

a2a-bench export --dataset demo-swarm --inputs <run>/bench-input --truth <run>/truth.jsonl \
  --out <out>/export-reference --split holdout --release reference@0.1.0
a2a-bench run --export <out>/export-reference --detector-cmd a2a-reference \
  --out <out>/reference --gates gates/reference.toml --examples 0 --holdout-release reference@0.1.0
```

The scenario totals sum each `report.json`'s `overall` counts and
`totals.exchanges` over the four runs of a scenario and derive the ratios from
the sums. The held-out run directories are not published; the commitment file
lets a later release check it scored the same runs and labels.
