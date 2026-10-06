```yaml
Overview:
  description: >
    a2a-transmission-bench is a neutral benchmark for detecting agent-to-agent
    transmissions in labelled multi-agent LLM traces. It converts public
    datasets into a provider-neutral on-disk format (messages, exchanges,
    labels), runs any detector as a separate process over the inputs, and
    scores the detector's predictions against the labels with regression
    gates per detector. Status: design approved (docs/design/separation.md);
    the format and corpus crates exist, the rest is being ported.
    the format and resource crates exist, the rest is being ported.
  subsystems:
    format: >
      a2a-bench-format. On-disk types for messages, exchanges, labels and
      predictions; checked constructors; JSONL IO; canonical JSON; part text;
      id derivation; format versioning. Every other crate depends on it.
    resource: >
      a2a-bench-resource. The neutral resource canonicaliser (URLs, forge
      repositories, repo files, threads, collections) used by converters for
      labels and by the scorer for predicted channels.
    corpus: >
      a2a-bench-corpus. The corpus model converters build on: the world
      builder (checked worlds), the virtual pace clock, streaming trace
      sources, the export writer and input view, datasets.toml and the
      dev/holdout splits, plus shared converter helpers.
      repositories, repo files, threads, collections, wiki pages) used by
      converters for labels and by the scorer, which canonicalizes labels and
      predictions alike. Version 1 equals crosstalk's extractor at 7f8a2fb;
      depends only on format and url.
    datasets: >
      a2a-bench-datasets. One converter per dataset (salt, agentdojo, tau2,
      collusion-wiki, swarm-traces, open-swe, lmcache, swe-splice, cipher,
      ai-village, demo-swarm), each a TraceSource built on corpus.
    reference: >
      a2a-bench-reference. The naive reference matcher, shipped as the
      a2a-reference detector binary.
    score: >
      a2a-bench-score. Alignment rule, judge, scorer, report, gates.
    cli: >
      a2a-bench-cli. The a2a-bench binary (export, validate, run, score, diff).
  data_flow: >
    dataset files -> converter (TraceSource of checked Worlds) ->
    corpus::export -> export dir (manifest, messages,
    exchanges, labels). The runner hands a detector only the input view
    (manifest without label counts, messages, exchanges); the detector writes
    predictions.jsonl. a2a-bench score reads labels, exchanges and
    predictions and writes report.json, a table and gate outcomes. Detectors
    are external processes; crosstalk's adapter (ct-bench-detect) lives in
    the crosstalk repo and depends only on a2a-bench-format.
Features Index:
  format:
    description: The on-disk format a2a-bench/1 - messages, exchanges, labels, predictions, manifest, framing, cross-file checks
    entry_points: [crates/format/src/lib.rs]
    depends_on: []
    doc: docs/features/format.md
  corpus:
    description: World builder, virtual clock, trace sources, export writer and input view, datasets.toml, dev/holdout splits, converter helpers
    entry_points: [crates/corpus/src/lib.rs, crates/corpus/src/export/mod.rs, crates/corpus/src/world/builder.rs]
    depends_on: [format]
    doc: docs/features/corpus.md
  resource:
    description: Neutral resource canonicaliser - canonical URLs, forge repositories, files, threads and collections, git remotes, canonicalize and kind; normative spec table and parity vectors from crosstalk 7f8a2fb
    entry_points: [crates/resource/src/lib.rs]
    depends_on: [format]
    doc: docs/features/resource.md
  dataset-demo-swarm:
    description: demo-swarm/headline and demo-swarm/boilerplate - crosstalk demo swarm runs labelled from the swarm's truth v2 over the adapter's bench capture (session + turn join, tool_use_id and BLAKE3 cross-checks, run window, typed join diagnostics)
    entry_points: [crates/datasets/demo-swarm/src/lib.rs, crates/datasets/demo-swarm/src/source.rs, crates/datasets/demo-swarm/src/label.rs]
    depends_on: [format, corpus, resource]
    doc: docs/features/dataset-demo-swarm.md
  separation:
    description: Design for splitting crosstalk-eval into this bench, the format, the detector contract, parity and versioning
    entry_points: []
    depends_on: []
    doc: docs/design/separation.md
```
