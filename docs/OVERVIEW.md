```yaml
Overview:
  description: >
    a2a-transmission-bench is a neutral benchmark for detecting agent-to-agent
    transmissions in labelled multi-agent LLM traces. It converts public
    datasets into a provider-neutral on-disk format (messages, exchanges,
    labels), runs any detector as a separate process over the inputs, and
    scores the detector's predictions against the labels with regression
    gates per detector. Status: design approved (docs/design/separation.md);
    the library crates are being integrated with the a2a-bench CLI.
  subsystems:
    format: >
      a2a-bench-format. On-disk types for messages, exchanges, labels and
      predictions; checked constructors; JSONL IO; canonical JSON; part text;
      id derivation; format versioning. Every other crate depends on it.
    resource: >
      a2a-bench-resource. The neutral resource canonicaliser (URLs, forge
      repositories, repo files, threads, collections, wiki pages) used by
      converters for labels and by the scorer, which canonicalizes labels and
      predictions alike. Version 1 equals crosstalk's extractor at 7f8a2fb;
      depends only on format and url.
    corpus: >
      a2a-bench-corpus. The corpus model converters build on: the world
      builder (checked worlds), the virtual pace clock, streaming trace
      sources, the export writer and input view, datasets.toml and the
      dev/holdout splits, plus shared converter helpers.
    datasets: >
      One crate per dataset under crates/datasets/<name>/ (package
      a2a-bench-dataset-<name>): salt, agentdojo, tau2, wiki (collusion-wiki),
      swarm (swarm-traces), open-swe, lmcache, swe-splice, cipher, ai-village,
      demo-swarm. Each is a TraceSource built on corpus.
    reference: >
      a2a-bench-reference. The naive reference matcher ("baseline detector
      0"), a library over one world's checked inputs and the a2a-reference
      detector binary (input view in, predictions.jsonl out). Attributes
      exchanges by client credential; depends on format only.
    score: >
      a2a-bench-score. Alignment rule, judge, scorer, report, gates.
    cli: >
      a2a-bench-cli. The a2a-bench binary (export, validate, run, score, diff).
  data_flow: >
    dataset files -> converter (TraceSource of checked Worlds) ->
    corpus::export -> export dir (manifest with per-world label counts and
    converter notes, messages, exchanges, labels). The runner hands a
    detector only the input view (manifest without label counts or notes,
    messages, exchanges); the detector writes
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
    description: World builder (with per-world notes), virtual clock, trace sources, export writer and input view, source digest IO, datasets.toml, dev/holdout splits, converter helpers
    entry_points: [crates/corpus/src/lib.rs, crates/corpus/src/export/mod.rs, crates/corpus/src/world/builder.rs]
    depends_on: [format]
    doc: docs/features/corpus.md
  resource:
    description: Neutral resource canonicaliser - canonical URLs, forge repositories, files, threads and collections, git remotes, canonicalize and kind; normative spec table and parity vectors from crosstalk 7f8a2fb
    entry_points: [crates/resource/src/lib.rs]
    depends_on: [format]
    doc: docs/features/resource.md
  score:
    description: Alignment rule, judge, scorer, report.json and table (full or holdout), per-detector gates, and the streaming entry point that scores an export against a predictions file
    entry_points: [crates/score/src/lib.rs, crates/score/src/run/mod.rs, gates/]
    depends_on: [format]
    doc: docs/features/score.md
  reference:
    description: Baseline detector 0 - naive span/shingle matching over a world's inputs, as a library and the a2a-reference binary
    entry_points: [crates/reference/src/lib.rs, crates/reference/src/bin/a2a-reference/main.rs]
    depends_on: [format]
    doc: docs/features/reference.md
  dataset-salt:
    description: SALT-NLP converter (dataset salt@1) - stratified trace discovery, per-episode call reconstruction on the pace clock, delivery labels with the forwarding tier, rejected-send, scripted-peer, shared-source and boilerplate controls; parity with ct-eval 7f8a2fb
    entry_points: [crates/datasets/salt/src/lib.rs, crates/datasets/salt/src/world.rs]
    depends_on: [format, corpus]
    doc: docs/features/dataset-salt.md
  separation:
    description: Design for splitting crosstalk-eval into this bench, the format, the detector contract, parity and versioning
    entry_points: []
    depends_on: []
    doc: docs/design/separation.md
```
