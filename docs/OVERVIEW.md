```yaml
Overview:
  description: >
    a2a-transmission-bench is a neutral benchmark for detecting agent-to-agent
    transmissions in labelled multi-agent LLM traces. It converts public
    datasets into a provider-neutral on-disk format (messages, exchanges,
    labels), runs any detector as a separate process over the inputs, and
    scores the detector's predictions against the labels with regression
    gates per detector. Status: design approved (docs/design/separation.md);
    the library crates and the a2a-bench CLI are integrated; parity stages
    P2-P4 passed on 2026-10-06 (results on test/parity).
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
      demo-swarm. Each is a TraceSource built on corpus with ct-eval's
      dataset id, an Options type (ct-eval's selection flags) and the files
      it read.
    reference: >
      a2a-bench-reference. The naive reference matcher ("baseline detector
      0"), a library over one world's checked inputs and the a2a-reference
      detector binary (input view in, predictions.jsonl out). Attributes
      exchanges by client credential; depends on format only.
    score: >
      a2a-bench-score. Alignment rule, judge, scorer, report (with the
      manifest's converter notes summed), gates. Canonicalises resources
      through a seam the CLI fills with a2a-bench-resource.
    cli: >
      a2a-bench-cli. The a2a-bench binary (export, validate, input-view, run,
      score, diff): dispatches to every converter crate, enforces revision
      pins and the holdout rules, runs detectors as processes and scores with
      the resource canonicaliser.
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
    description: The on-disk format a2a-bench/1 - messages, exchanges, labels, predictions, manifest, framing, cross-file checks (including location-sorted evidence in predictions)
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
  dataset-agentdojo:
    description: AgentDojo converter (agentdojo@1) - prompt-injection runs as a victim and a synthetic attacker; injection copies (construction, channel copies out of reach), boilerplate controls; ct-eval 7f8a2fb parity
    entry_points: [crates/datasets/agentdojo/src/lib.rs, crates/datasets/agentdojo/src/source.rs]
    depends_on: [format, corpus]
    doc: docs/features/dataset-agentdojo.md
  dataset-tau2:
    description: tau2-bench converter (tau2@1) - simulations as an agent and a user simulator with flipped views at recorded times; structural turn labels, boilerplate and shared-source controls; ct-eval 7f8a2fb parity
    entry_points: [crates/datasets/tau2/src/lib.rs, crates/datasets/tau2/src/source.rs]
    depends_on: [format, corpus]
    doc: docs/features/dataset-tau2.md
  dataset-wiki:
    description: collusion-wiki converter (collusion-wiki@1) - wiki revisions as synthesised HTTP-tool exchanges, one world per agent-page component, heuristic channel transmissions, reread controls and relays; parity with ct-eval 7f8a2fb
    entry_points: [crates/datasets/wiki/src/lib.rs, crates/datasets/wiki/src/build/mod.rs]
    depends_on: [format, corpus, resource]
    doc: docs/features/dataset-wiki.md
  dataset-swarm:
    description: swarm-traces converter (swarm-traces@1) - encoded payload tokens decoded through codec chains, one two-agent world per token with a decoded-class direct label; no payload text in logs or errors; parity with ct-eval 7f8a2fb
    entry_points: [crates/datasets/swarm/src/lib.rs, crates/datasets/swarm/src/build.rs]
    depends_on: [format, corpus]
    doc: docs/features/dataset-swarm.md
  dataset-demo-swarm:
    description: demo-swarm/headline and demo-swarm/boilerplate - crosstalk demo swarm runs labelled from the swarm's truth v2 over the adapter's bench capture (session + turn join, tool_use_id and BLAKE3 cross-checks, run window, typed join diagnostics); the export only adds truth, keeping the capture's manifest as its input view so the adapter's predictions score; a holdout export (a whole run) marks it split holdout with the release and the capture's digest
    entry_points: [crates/datasets/demo-swarm/src/lib.rs, crates/datasets/demo-swarm/src/source.rs, crates/datasets/demo-swarm/src/label.rs]
    depends_on: [format, corpus, resource]
    doc: docs/features/dataset-demo-swarm.md
  dataset-ai-village:
    description: AI Village converter (ai-village@1) - the Claude Code stream (construction-tier get_events labels) and village-day windows (structural chat, heuristic repository channel labels), with the bench's own normative shell model (command table, shell state, write outcomes) for bash accesses; parity with ct-eval at crosstalk 7f8a2fb proven by an access agreement check and a label comparison
    entry_points: [crates/datasets/ai-village/src/lib.rs, crates/datasets/ai-village/src/shell/mod.rs]
    depends_on: [format, corpus, resource]
    doc: docs/features/dataset-ai-village.md
  dataset-open-swe:
    description: open_swe@1 - Open-SWE-Traces trajectories mixed into background worlds (no positives, controls per pair); shard discovery and round-robin rows reused by swe-splice
    entry_points: [crates/datasets/open-swe/src/lib.rs, crates/datasets/open-swe/src/source.rs]
    depends_on: [corpus, format]
    doc: docs/features/dataset-open-swe.md
  dataset-lmcache:
    description: lmcache@1 - LMCache agentic sessions as background worlds; cumulative requests rebuilt into calls, recorded pre_gap times
    entry_points: [crates/datasets/lmcache/src/lib.rs, crates/datasets/lmcache/src/source.rs]
    depends_on: [corpus, format]
    doc: docs/features/dataset-lmcache.md
  dataset-swe-splice:
    description: swe_splice@1 - seeded splices of a file write into another Open-SWE trajectory's later read (four variants, editor view or shell cat), one channel label per world
    entry_points: [crates/datasets/swe-splice/src/lib.rs, crates/datasets/swe-splice/src/world.rs]
    depends_on: [dataset-open-swe, corpus, format]
    doc: docs/features/dataset-swe-splice.md
  dataset-cipher:
    description: cipher@1 - seeded sender/receiver pairs carrying a payload under eight encoders; rot13, rotN, binary8 and substitution are out of reach by design
    entry_points: [crates/datasets/cipher/src/lib.rs, crates/datasets/cipher/src/world.rs]
    depends_on: [corpus, format]
    doc: docs/features/dataset-cipher.md
  cli:
    description: The a2a-bench binary - export (dispatch to every converter, source digest, revision detection (HF snapshot, HF --local-dir metadata, git HEAD, else unknown) and pinning, dev/holdout splits, holdout commitment; demo-swarm holdouts are whole runs with a seed of at least 1,000,000, committed one line per run), validate (every format check, counts only), input-view, run (detector contract, --twice determinism), score (resource canonicaliser, gates, exit 2), diff (exports or predictions row by row; detector header fields reported apart; --normalize-ids names detector agents by their exchange sets and drops transmission ids, for P3)
    entry_points: [crates/cli/src/bin/a2a-bench/main.rs, crates/cli/src/lib.rs, crates/cli/src/datasets/dispatch.rs]
    depends_on: [format, corpus, resource, score, dataset-salt, dataset-agentdojo, dataset-tau2, dataset-wiki, dataset-swarm, dataset-open-swe, dataset-lmcache, dataset-swe-splice, dataset-cipher, dataset-ai-village, dataset-demo-swarm]
    doc: docs/features/cli.md
  separation:
    description: Design for splitting crosstalk-eval into this bench, the format, the detector contract, parity and versioning
    entry_points: []
    depends_on: []
    doc: docs/design/separation.md
```
