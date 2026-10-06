```yaml
Overview:
  description: >
    a2a-transmission-bench is a neutral benchmark for detecting agent-to-agent
    transmissions in labelled multi-agent LLM traces. It converts public
    datasets into a provider-neutral on-disk format (messages, exchanges,
    labels), runs any detector as a separate process over the inputs, and
    scores the detector's predictions against the labels with regression
    gates per detector. Status: design approved (docs/design/separation.md);
    the format crate and the reference matcher exist, the rest is being
    ported.
  subsystems:
    format: >
      a2a-bench-format. On-disk types for messages, exchanges, labels and
      predictions; checked constructors; JSONL IO; canonical JSON; part text;
      id derivation; format versioning. Every other crate depends on it.
    resource: >
      a2a-bench-resource. The neutral resource canonicaliser (URLs, forge
      repositories, repo files, threads, collections) used by converters for
      labels and by the scorer for predicted channels.
    datasets: >
      a2a-bench-datasets. One converter per dataset (salt, agentdojo, tau2,
      collusion-wiki, swarm-traces, open-swe, lmcache, swe-splice, cipher,
      ai-village, demo-swarm), the world builder and the virtual pace clock.
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
    dataset files -> a2a-bench export -> export dir (manifest, messages,
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
  reference:
    description: Baseline detector 0 - naive span/shingle matching over a world's inputs, as a library and the a2a-reference binary
    entry_points: [crates/reference/src/lib.rs, crates/reference/src/bin/a2a-reference/main.rs]
    depends_on: [format]
    doc: docs/features/reference.md
  separation:
    description: Design for splitting crosstalk-eval into this bench, the format, the detector contract, parity and versioning
    entry_points: []
    depends_on: []
    doc: docs/design/separation.md
```
