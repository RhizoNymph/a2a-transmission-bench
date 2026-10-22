# a2a-transmission-bench

A neutral benchmark for one task: given labelled multi-agent LLM traces,
find the agent-to-agent transmissions in them. Any detector can be scored.
crosstalk is one detector,
plugged in through its own adapter (`ct-bench-detect`, in crosstalk's
`crosstalk-bench-adapter` crate).

The bench converts public multi-agent datasets (SALT, AgentDojo,
τ²-bench, collusion-wiki, swarm-traces, AI Village, open-swe, lmcache, and
synthetic SWE splice and cipher corpora) into one provider-neutral format,
runs a detector as a separate process over the inputs only, and scores its
predictions against the labels with per-detector regression gates.
Dataset bytes are never committed; they are read from a data root
(`~/Data/ai/agents` by default, see `datasets.toml`).

## Use

```text
cargo build --release -p a2a-bench-cli --all-features

# convert a dataset selection into an export (messages, exchanges, labels, manifest)
a2a-bench export --dataset salt --limit 53 --out exports/salt

# run a detector on the export's input view, then score it against the labels and gates
a2a-bench run --export exports/salt --detector-cmd "a2a-reference" --out runs/salt-reference

# or score a predictions file a detector wrote elsewhere
a2a-bench score --export exports/salt --predictions predictions.jsonl --out runs/salt-other
```

`run` and `score` exit 2 when a gate fails. Gates live in `gates/`, one
file per detector.

A detector is any program called as
`<detector> --input <dir> --output <predictions.jsonl>`, where `<dir>`
holds `manifest.json`, `messages.jsonl` and `exchanges.jsonl` and never
the labels. `a2a-reference`, the bench's naive baseline, is the first.

## Results

- [docs/results/release-2026-10.md](docs/results/release-2026-10.md):
  the first held-out demo-swarm release (six runs, crosstalk `7d678e2`,
  bench `a2a-bench-v1.0.1`, `demo-swarm@1`) and the public-dataset (dev)
  table (crosstalk `6ba4bd8`)
- [docs/results/release-2026-10-r2.md](docs/results/release-2026-10-r2.md):
  the second held-out demo-swarm release (eight runs, crosstalk
  `a74f808`, bench `a2a-bench-v1.1.1`, `demo-swarm@2`); the dev table is
  not rerun

## Versions

- Bench releases are tagged `a2a-bench-v<x.y.z>`; the current one is
  `a2a-bench-v1.1.1` (`114217d`). Results name the tag they were
  scored at.
- The format crate is tagged separately: `a2a-bench-format-v1.0.0`
  (`c289eda`, format `a2a-bench/1`), the tag crosstalk's adapter pins.
- The crates' Cargo version is the workspace's `0.1.0` (the
  `converter.crate_version` in a manifest, and `a2a-reference`'s
  version); it is not bumped with the tags. Dataset versions
  (`<dataset>@<n>`) are per converter: every converter is at `@1`
  except demo-swarm, at `@2`.

## Docs

- [docs/OVERVIEW.md](docs/OVERVIEW.md): subsystems and the features index
- [docs/features/format.md](docs/features/format.md): the `a2a-bench/1` format
- [docs/features/cli.md](docs/features/cli.md): the `a2a-bench` commands
- [docs/design/separation.md](docs/design/separation.md): why and how the
  bench was split from crosstalk's evaluation harness, including the
  holdout split
- [parity/results/README.md](parity/results/README.md): parity with
  crosstalk-eval, stages P1–P7
