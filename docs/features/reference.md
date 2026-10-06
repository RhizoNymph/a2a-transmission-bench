# Reference matcher (`a2a-bench-reference`)

The bench's "baseline detector 0": deliberately naive span and shingle
matching, ported from crosstalk-eval's `reference` module (crosstalk
`7f8a2fb`). It sets a floor any detector should beat and validates labels:
a construction-tier label it cannot find is worth a look. It ships as a
library over one world's checked inputs and as the `a2a-reference`
binary, the first detector to satisfy the detector contract
([../design/separation.md](../design/separation.md) §4). It depends on
`a2a-bench-format` only, never on a crosstalk crate.

## Scope

- Matching one world's inputs (`a2a_bench_format::check::WorldInputs`):
  originated spans, shingle index with a boilerplate cutoff, folding,
  classification, decoding, opaque blobs, routes, rereads.
- Turning hits into format `Prediction` rows: one `attribution` row per
  credential and one confirmed `transmission` per (reader exchange,
  sender, route), with its strongest match's quality.
- A per-world summary on the side (spans, matches, rereads, out of reach),
  logged by the binary.
- The `a2a-reference` binary: input view in, one predictions file out,
  byte-identical across runs.

## Non-scope

- Identity inference. The reference attributes by credential (below);
  `unattributed` rows are never written.
- Co-access evidence (`suspected`/`discarded` transmissions), semantic
  matches, nested decoding (base64 inside base64, two string levels).
- Resource canonicalisation beyond the URL and path normalisation it
  needs to key channels; the scorer canonicalises resources again.
- Scoring, labels, gates: the reference never reads `labels.jsonl`.

## Data and control flow

### Binary

```text
a2a-reference --input <dir> --output <predictions.jsonl> [--max-postings N]

manifest.json ──serde──▶ Manifest ──▶ manifest_digest = Manifest::digest()
messages.jsonl ─FileReader<Messages>──┐  (headers' dataset == manifest.dataset)
exchanges.jsonl ─FileReader<Exchanges>┤
                                      ▼
Worlds::next_world: both files' next_world() in lockstep;
  one ends before the other, keys differ, or the manifest's worlds[i] differs ⇒ run failure
                                      ▼
WorldFiles::into_inputs ──▶ WorldInputs::new   (InputError ⇒ world failed {reason})
                                      ▼
a2a_bench_reference::run(inputs, config)       (ReferenceError ⇒ world failed {reason})
                                      ▼
check_predictions(inputs, rows)                (PredictionError ⇒ world failed {reason})
                                      ▼
FileWriter<Predictions>: header {detector {name: reference, version: crate version,
  variant: max-postings-<N>, config_digest}, manifest_digest}
  · world {key, status} · rows · … · trailer
```

A run failure exits 1 and leaves the file without a trailer, which every
reader refuses. Logs go to stderr as structured `tracing` events
(`RUST_LOG`, default `info`): one `world scored` event per world with its
summary fields, `world failed` with the reason, `predictions written` with
the trailer's counts and digest.

### Library (`run`)

```text
Agents::of(inputs)            one detector agent per distinct client.credential, numbered by first exchange
for each exchange (world order), reader = its credential's agent:
  new_inputs(previous of reader, exchange)       delta.rs: request ids beyond the previous request + response (multiset)
  for each new non-assistant message, each part with text:
    scan                                          matcher/scan.rs
      segments(text)                              text/opaque.rs: pieces between opaque blobs
      fold(piece) → shingles(k) → lookup          spans of other agents sharing windows
      covered runs ≥ min_span, ≥ min_word_chars → classify_forms (text/classify.rs)
        Match(kind) | TwoStringLevels (None: counted out_of_reach unless another reading is in reach)
      reader's seen += piece's windows
      decode_candidates(piece) → fold → shingles → lookup; longest run ≥ min_span ⇒ decoded {codec}
      per (span, raw range): first in-reach kind; route_of(message, part) → carrier, route, route key
  first_reads(reader, hits)                       matcher/group.rs: drop channel rereads
  group(exchange, hits)                           one transmission per (sender, route key)
    transmission_ref + confirmed(strongest)       predict.rs
  for each response message: index_response      matcher/index.rs: originated spans, postings
  previous[reader] = exchange
predictions = attributions (by agent name) ++ transmissions (production order)
```

### Matching, in order

1. **Folding** (`text/fold.rs::fold`): string escapes unfolded at any depth
   (`\n \t \r \b \f` become whitespace, `\uXXXX` and surrogate pairs their
   character, a YAML `\`-newline drops the break and the indentation, any
   other escape drops its backslashes), then case folded, then whitespace
   runs collapsed to one space with leading whitespace dropped. Each folded
   byte keeps the raw range it came from.
2. **Spans** (`matcher/index.rs`): in a response part, the folded pieces'
   windows the agent has not seen; each covered run at least `min_span`
   (raised to `k`) folded bytes and `min_word_chars` letters/digits long
   is a span. Its windows not already seen are posted. Then every window of
   the part is seen by the agent.
3. **Boilerplate cutoff**: a shingle posted for more than `max_postings`
   distinct spans (default `MAX_POSTINGS` = 50, L4's `IndexSettings`
   cutoff) turns `Boilerplate`: its postings are dropped and it is never
   indexed or looked up again in the world. Only originated spans count
   toward the frequency; there is no retention window.
4. **Lookup** (`matcher/scan.rs`): spans of agents other than the reader
   sharing windows with the read; each covered run passing the same two
   minimums is a candidate over its raw range.
5. **Classification** (`text/classify.rs`), in this order: `exact` (the span
   holds the read bytes); `normalized` (equal under case and whitespace
   folding alone, `fold_plain`); `decoded [json_string | yaml_string]`
   (equal once one string level is undone, `unescape_once`, on the read
   side, a YAML `''` undone on the read side, or the span side; the codec
   is the one whose escapes the undone text holds, `string_codec`);
   `TwoStringLevels` (equal only once exactly two levels are undone on one
   side: out of reach, counted, no match); otherwise a bridged range,
   classed by its escapes as one string level.
6. **Decoding** (`text/decode.rs`): hex runs (even length), base64 runs
   (standard and URL-safe, padded or not; not all-hex) and URL-encoded
   tokens (three or more `%`), at least `min_decoded` raw bytes, that decode
   to UTF-8 at least 90% printable. A token whose decoded text shares a
   covered run of at least `min_span` with a span is a `decoded [codec]` hit
   over the token's raw range.
7. **Opaque blobs** (`text/opaque.rs`): Gemini thought-signature call ids
   (`…__thought__<base64>`) and the string or string-array values of
   `thought_signature`, `thought_signatures`, `signature`,
   `encrypted_content` and `redacted_thinking` members (escaped or not)
   are cut out before spans, matching and decoding.
8. **Routes** (`route.rs`): a system message is carrier `system_prompt`,
   route `direct`; a user message `user_turn`, `direct`; a tool result
   `tool_result`, and `channel {resource}` when the request's call with that
   call id (the last one) names a resource, else `direct`. Resources: the
   first string argument, in member order, that is an `http(s)` URL
   (`Resource::Url`, scheme and host lower-cased, default port and fragment
   dropped, query parameters sorted), or an absolute path under a path-like
   name (`path`, `file`, `file_path`, `filepath`, `filename`, `target`,
   `source_path`, `target_path`; `Resource::File`, `.`/`..` resolved). The
   reference never writes `Resource::Opaque` or forge resources.
9. **Rereads** (`matcher/group.rs`): a channel hit on a span already
   reported to the same reader through the same channel key in an earlier
   exchange is dropped (INV-1122). Hits on one span in one exchange all
   count; direct routes are never rereads.
10. **Grouping**: one transmission per (sender, route key), route keys
    `direct:system_prompt`, `direct:user_turn`, `direct:tool:<name>`
    (`unknown` without a call) and `channel:<resource key>`. Two tools'
    results are two transmissions even though both rows say `direct`.

### Prediction rows (`predict.rs`, `agents.rs`)

- **Attribution**: the reference has no identity inference, so it
  attributes each exchange to the detector agent named by its
  `client.credential` (one agent per credential, the credential text as
  its name). This is the reference's own naive choice. Agents sharing a
  credential are one detector agent; what they pass each other is a
  self-read and never reported, and the scorer sees a merge if the
  credential spans two true agents. A credential that is not a valid
  `DetectorAgent` (empty, or holding a control character) fails the world.
- **Transmission**: state `confirmed`, `matches` in hit order, each
  `ContentEvidence {from, to, reader_exchange, read_at, origin_at, match,
  carrier, route}` with `origin_at` the span's location in the sender's
  response.
- **Quality**: crosstalk-spec's `MatchClass::strongest_match` (INV-519):
  the strongest class among the matches in the order `exact`,
  `normalized`, `decoded`, `semantic`, with the carrier of the first match
  of that class in stored order (`predict::strongest`).
- **Id**: `t:` and the hex BLAKE3, derive-key context
  `a2a-bench-reference/1 transmission`, of reader exchange ULID, sender
  name and route key, each followed by a zero byte.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `crates/reference/Cargo.toml` | crate and the `a2a-reference` binary | |
| `src/lib.rs` | crate root and algorithm overview | `run`, `ReferenceConfig`, `MAX_POSTINGS`, `ReferenceError`, `WorldOutput`, `WorldSummary` |
| `src/config.rs` | matcher parameters | `ReferenceConfig` (`k` 24, `min_span` 24, `min_decoded` 16, `min_word_chars` 20, `max_postings` 50), `MAX_POSTINGS`, `ReferenceConfig::effective` |
| `src/error.rs` | why a world could not be processed | `ReferenceError` (`Credential`, `MissingMessage`, `TransmissionRef`, `Transmission`) |
| `src/output.rs` | one world's result | `WorldOutput {predictions, summary}`, `WorldSummary` |
| `src/agents.rs` | one detector agent per credential | `Agents::of`, `assignments`, `name`, `attributions` |
| `src/delta.rs` | new inputs of an exchange | `new_inputs` |
| `src/route.rs` | carriers' resources | `find_call`, `extract_resource`, `parse_url`, `normalize_path`, `Url`, `ChannelResource` |
| `src/predict.rs` | ids, strongest match, checked transmissions | `strongest`, `transmission_ref`, `confirmed`, `TRANSMISSION_ID_CONTEXT` |
| `src/matcher/mod.rs` | the run loop and its state | `run` (re-exported) |
| `src/matcher/index.rs` | originated spans, postings and the boilerplate cutoff | (private) `Postings`, `index_response` |
| `src/matcher/scan.rs` | scanning a part, lookup, routes of hits | (private) `scan`, `lookup`, `route_of` |
| `src/matcher/group.rs` | rereads and grouping | (private) `first_reads`, `group` |
| `src/text/fold.rs` | the matching fold and its helpers | `fold`, `Folded`, `fold_plain`, `unescape_once`, `string_codec` |
| `src/text/classify.rs` | a hit's class | `classify`, `classify_forms`, `Classified`, `SpanForms`, `unescaped_plain` |
| `src/text/decode.rs` | base64, hex and URL decoding of tokens | `decode_candidates`, `Decoded` |
| `src/text/opaque.rs` | opaque blobs | `opaque_ranges`, `segments` |
| `src/text/shingle.rs` | rolling-hash shingles and covered runs | `shingles`, `covered` |
| `src/bin/a2a-reference/main.rs` | entry point, logging, exit code | |
| `src/bin/a2a-reference/args.rs` | clap arguments | `Args` |
| `src/bin/a2a-reference/inputs.rs` | the input view, world by world in lockstep | `manifest`, `Worlds`, `WorldFiles` |
| `src/bin/a2a-reference/detect.rs` | the run and the predictions file | `run`, `NAME`, `CONFIG_DIGEST_CONTEXT` |
| `tests/common/mod.rs` | synthetic worlds | `WorldBuilder`, message helpers, `matched` |
| `tests/matcher.rs` | the matcher on hand-built worlds (ported from ct-eval) | |
| `tests/text.rs` | fold, classify, decode, shingle, opaque, route unit tests (ported) | |
| `tests/predictions.rs` | attribution, quality, ids | |
| `tests/binary.rs` | the binary against the detector contract | |

## Invariants and constraints

- **Parity with crosstalk-eval's reference.** Every matching rule above is
  ct-eval's at `7f8a2fb`: `MAX_POSTINGS` 50, the fold and classify order,
  the two-string-levels out-of-reach count, first reads, the opaque
  exclusions, `min_span` raised to `k`. Differences are only in identity,
  ids and the row shapes (below).
- **No truth.** The reference reads the input view only; it never sees
  `exchange_agent` rows.
- **Worlds are independent.** All state (index, seen sets, delivered set)
  is built per world.
- **Determinism.** The same inputs give the same rows and the same file
  bytes: hash maps are only probed, never iterated into output; hits,
  groups and attributions are ordered by `BTreeMap`s and world order
  (`two_runs_are_byte_identical`).
- **Checked output.** Every world's rows pass `check_predictions` before
  they are written; a world whose rows would not is `failed`.
- **Deviations from ct-eval**, all forced by the bench boundary:
  - agents are credentials, not the dataset's agents, so per-agent state
    (previous request, seen windows) is per credential, and agents sharing
    a credential never match each other;
  - an exchange's response may hold several messages; each is indexed in
    order, and each counts as echoed in the next request;
  - transmission ids are the reference's own (above), and span ids are
    gone: evidence carries the span's location as `origin_at`;
  - a `direct` route carries no tool name (the format has none); the
    grouping key still does, so groups are ct-eval's;
  - `WorldSummary::rereads` is new; `matches` counts rereads, as ct-eval's
    did.
- **Limits.** No unwrap, expect or panic outside tests; no unsafe; modules
  stay well under 1000 lines.
