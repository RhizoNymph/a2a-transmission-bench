# Release results, 2026-10

crosstalk's live detector against the bench's naive baseline `a2a-reference`,
both scored by a2a-transmission-bench on the public datasets' parity selections.

- **Bench:** `ddccf4f7475ac6c44a362b4b429e3d82611f4852` (`git describe`:
  `a2a-bench-format-v1.0.0-2-gddccf4f`), `a2a-bench` and `a2a-reference` built with
  `cargo build --release -p a2a-bench-cli -p a2a-bench-reference --all-features`
  (`rustc 1.101.0-nightly (c36f14571 2026-10-01)`). Every export manifest records
  converter `0.1.0` at this commit.
- **crosstalk:** `6ba4bd849dca40daabe1701b0073345b2b0367cc` (staging), adapter
  `ct-bench-detect`. Its predictions headers read `detector.name = crosstalk-live`,
  `detector.version = 6ba4bd849dca40daabe1701b0073345b2b0367cc`,
  `detector.variant = forwarding-off`.
- **Reference:** `a2a-reference` `0.1.0`, variant `max-postings-50`.
- **Date:** 2026-10-06 (runs 19:35 to 20:04 PDT), one machine, one detector at a time.
- **Dataset sources** (from each export's `manifest.json`: `source.revision`, and
  `source.digest`, the bench's digest of the files its converter read):

| dataset | source revision | source digest |
| --- | --- | --- |
| collusion-wiki | `unknown` (plain files, no VCS) | `2e1e6e11f879cdd2bd19f769b5ff9ec95d3d39e64634a8120d00210f525bf336` |
| swarm-traces | `unknown` (plain file, no VCS) | `105f2565f8873e5483e9ec977ac7c0873b93b926fd5e00e224760b2826d40a31` |
| cipher | `e885f4aa4fd049aa98a51e8a57368bdda237db78` | `ea445c4fc6d7e25e7f361400c6f7c176c33aa8e55b9a0226e5a678110b664fa4` |
| salt | `2eba8f3771e8fbcc0f49f6cbbfd2111b939a117a` | `1e6a78aadd2f54562ea7bf5bd3c4f73d2a2b48a4efda50ad84232bfd6e1f130f` |
| agentdojo | `089ed468cf3ed0322acc66b0211f26d9d90dbf60` | `ca0a72361d933dd28995c31cd6cc09939d17b5183f478d6889b15e660e592a4b` |
| swe_splice, open_swe | `f8fb5b3d2c787f85f8a00f5fe04fe3f1a11088ef` | `53f10a122b1041fda47ede2d61750035d19b918057692153fa0a9a906a6a4495` |
| lmcache | `6e043b9e89865df3aec19fd5679286b683bfd70e` | `bee783f324c18b2d5b81da46ecbe60623a0c1b833a1fad76c5c01798c9ff7364` |
| tau2 | `5bfa7e37b36656b37dc6d022156be6563c1007f3` | `73ff73a7e34d9bb888859862baa300a0257d67d3be7fbdf41599b98a3371ad45` |
| ai-village | `838b4150303ca8228e8edb432d8b8ccae353d258` | `e60ee480e383ce323d7d4d5e4f5a06d1769cd58d574c90b6b87938b85b08b6f4` |

## Public datasets (dev data: crosstalk was tuned on these)

**These are development numbers, not held-out ones.** crosstalk was developed and
tuned on every dataset below, and its gates were set from earlier runs on the
same selections. The reference detector was not tuned to them.

| dataset / selection (dev data) | worlds | exchanges | labels (in reach) | crosstalk recall | crosstalk precision | crosstalk FP/1k | reference recall | reference precision | reference FP/1k | crosstalk gates |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| collusion-wiki `--demo` | 5 | 156 | 134 | 0.993 (133/134) | 1.000 (131/131) | 0.0 | 1.000 (134/134) | 1.000 (181/181) | 0.0 | 2/2 pass |
| collusion-wiki `--max-agents 100` | 590 | 1,982 | 100 | 0.960 (96/100) | 1.000 (116/116) | 0.0 | 1.000 (100/100) | 1.000 (416/416) | 0.0 | 2/2 pass |
| swarm-traces (full) | 629 | 1,887 | 629 | 1.000 (629/629) | 1.000 (629/629) | – | 0.997 (627/629) | 1.000 (1,261/1,261) | – | 2/2 pass |
| cipher `--count 50` | 400 | 1,000 | 200 | 0.245 (49/200) | 1.000 (49/49) | – | 0.430 (86/200) | 1.000 (86/86) | – | none apply (ungated) |
| SALT `--limit 53` | 53 | 11,796 | 2,909 | 0.855 (2,486/2,909) | 0.956 (3,522/3,686) | 13.9 | 0.971 (2,824/2,909) | 0.583 (6,398/10,978) | 388.3 | 5/5 pass |
| AgentDojo (documented selection) | 2,259 | 11,235 | 2,324 | 0.996 (2,315/2,324) | 0.975 (3,609/3,701) | 8.2 | 1.000 (2,324/2,324) | 0.811 (3,172/3,912) | 65.9 | 3/3 pass |
| swe-splice `--count 80` | 80 | 11,780 | 80 | 1.000 (80/80) | 0.939 (642/684) | 3.6 | 0.738 (59/80) | 0.717 (3,543/4,944) | 118.9 | 2/2 pass |
| open-swe `--count 16` (background only) | 13 | 14,311 | 0 | – | – (0 correct, 208 false) | 14.5 | – | – (0 correct, 4,539 false) | 317.2 | 1/1 pass (FP/1k ≤ 30) |
| lmcache `--count 16` (background only) | 5 | 2,513 | 0 | – | – (0 correct, 293 false) | 116.6 | – | – (0 correct, 4,728 false) | 1,881.4 | 1/1 pass (FP/1k ≤ 150) |
| τ² (full) | 10,832 | 264,793 | 119,256 | 0.993 (118,422/119,256) | 0.999 (186,219/186,320) | 0.4 | 0.999 (119,085/119,256) | 0.998 (241,233/241,794) | 2.1 | 2/2 pass |
| AI Village, Claude Code mode (all contexts) | 993 | 81,369 | 15,798 | 0.998 (15,767/15,798) | 1.000 (38,579/38,579) | – | 0.999 (15,776/15,798) | 1.000 (73,754/73,754) | – | 1/1 pass |

Notes:

- **In reach** means a label the detector could find from the exchanges it is
  given. Recall is `found / expected` over in-reach labels only (the report's
  `overall`). Out-of-reach and forwarding labels are scored apart and are not in
  the table:
  - AgentDojo: 359 out-of-reach labels (copies read through `get_webpage` or
    `read_file`, which the synthetic attacker never writes). crosstalk 0/359,
    reference 87/359.
  - cipher: 200 out-of-reach labels. crosstalk 0/200, reference 0/200.
  - SALT: 941 forwarding labels (deliveries that paste the sender's own tool
    output), known misses with forwarding off, which is how crosstalk ships and how
    it ran here. crosstalk 462/941 (0.491), reference 738/941 (0.784).
- **Precision** is `correct / (correct + false)`. Predictions the scorer marks
  unjudged are left out: those that align with no label and fall under an
  exemption or a world whose label coverage is only partial (see
  `docs/features/score.md`, "Judging"). Unjudged counts:
  collusion-wiki `--demo` 12 crosstalk and 189 reference, `--max-agents 100` 9 and 38,
  AI Village 29,649 and 264,503. AI Village's precision therefore counts only
  the predictions the labels decide. Predictions crosstalk itself discards and
  the scorer dismisses (collusion-wiki `--demo`: 52) are not charged either.
- **FP/1k** is false positives per 1,000 exchanges, given only where the dataset
  has negative controls (`–` otherwise). open-swe and lmcache are background only:
  they have no transmission labels, every prediction is a false positive, and
  FP/1k is their metric.
- **Access-only recall** (labels found only by a suspected or discarded
  prediction, already inside recall) is nonzero once: collusion-wiki
  `--max-agents 100`, crosstalk 4/100 (0.040). It is 0 everywhere else, for both
  detectors.
- **Gates** count the gates in `gates/crosstalk-live.toml` that apply to the
  dataset (n pass / m applied). They are regression gates set a little below
  earlier dev results on these same selections, not an independent bar. cipher
  has no crosstalk gate. Reference runs used `gates/reference.toml`: SALT 5/5
  and AgentDojo 2/2 and τ² 2/2 pass, none apply elsewhere.
- No run exited non-zero, no world failed and none went unscored.
- The reference numbers equal the bench's parity results
  (`parity/results/README.md`, P2/P3) count for count on every dataset there.
  The remaining reference rows (wiki `--max-agents 100`, swe-splice, open-swe,
  lmcache, AI Village) equal ct-eval's own reference reports at crosstalk 7f8a2fb
  (the parity baseline) at the three decimals those reports give.
- Every crosstalk run logs `WARN crosstalk_gateway::live::evidence: announced span
  not in the span source` lines (583,578 across all runs, no `ERROR` lines), as
  the parity baseline's live runs did. They are not world failures.

Wall times (seconds; shared 16-core machine, load about 6 to 10):

| dataset / selection (dev data) | export | crosstalk run | reference run |
| --- | ---: | ---: | ---: |
| collusion-wiki `--demo` | 0.2 | 0.2 | 0.05 |
| collusion-wiki `--max-agents 100` | 0.2 | 1.6 | 0.2 |
| swarm-traces | 1.9 | 0.9 | 0.1 |
| cipher `--count 50` | 0.05 | 0.4 | 0.06 |
| SALT `--limit 53` | 13.7 | 97.9 | 7.5 |
| AgentDojo | 4.3 | 21.5 | 2.3 |
| swe-splice `--count 80` | 20.2 | 68.8 | 6.7 |
| open-swe `--count 16` | 9.4 | 96.2 | 10.8 |
| lmcache `--count 16` | 7.0 | 15.7 | 1.9 |
| τ² | 15.5 | 558.7 | 54.1 |
| AI Village, Claude Code mode | 23.3 | 537.4 | 50.7 |

## Reproduce

From a bench checkout at `ddccf4f`, with `CT` the `ct-bench-detect` binary built
from crosstalk `6ba4bd8` and `CTX` that crosstalk checkout, and the datasets under
`~/Data/ai/agents` (the root in `datasets.toml`):

```sh
cargo build --release -p a2a-bench-cli -p a2a-bench-reference --all-features
B=target/release/a2a-bench
REF=target/release/a2a-reference
R=target/release-run

# one dataset: export, then run each detector over the same export
run() {
  d=$1; ctflags=$2; shift 2
  $B export "$@" --out $R/$d/export
  $B run --export $R/$d/export --detector-cmd "$CT $ctflags" \
    --out $R/$d/crosstalk --gates gates/crosstalk-live.toml --examples 0
  $B run --export $R/$d/export --detector-cmd "$REF" \
    --out $R/$d/reference --gates gates/reference.toml --examples 0
}

run wiki-demo "" --dataset collusion-wiki --demo
run wiki-max-agents-100 "" --dataset collusion-wiki --max-agents 100
run swarm-traces "" --dataset swarm-traces
run cipher-50 "" --dataset cipher --count 50
run salt-53 "" --dataset salt --limit 53
run agentdojo "--extract-config $CTX/crates/eval/extract/agentdojo.json" \
  --dataset agentdojo \
  --include pipeline=gpt-4o-2024-05-13 --include pipeline=claude-3-5-sonnet-20241022 \
  --include pipeline=gemini-1.5-pro-002 \
  --include attack=important_instructions --include attack=none
run swe-splice-80 "" --dataset swe_splice --count 80
run open-swe-16 "" --dataset open_swe --count 16
run lmcache-16 "" --dataset lmcache --count 16
run tau2 "" --dataset tau2
run ai-village-claude-code "" --dataset ai-village --mode claude-code
```

`ct-bench-detect` runs with its defaults otherwise: live mode, forwarding off,
60 s correlation window, 10 s evidence window, 60 s suspected TTL, seed 0. These
mirror ct-eval's live settings in the parity baseline. Each run directory holds
`report.txt` and `report.json`.

## Held-out results (fresh demo-swarm runs)

pending
