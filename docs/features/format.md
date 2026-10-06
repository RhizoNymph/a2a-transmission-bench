# On-disk format (`a2a-bench-format`)

The normative definition of format `a2a-bench/1`: the files an export and a
detector run consist of, their rows, and the checks every reader applies.
The crate `a2a-bench-format` (`crates/format`) is its only implementation;
everything else in the bench, and crosstalk's adapter, reads and writes
through it. The design and its reasons are in
[../design/separation.md](../design/separation.md) §3; where they differ,
this doc and the crate win.

## Scope

- Types for messages, exchanges, labels, predictions and the manifest,
  with checked constructors; deserialization goes through the same checks.
- Canonical JSON (RFC 8785 with exact numbers) and part text, both equal
  to crosstalk-spec's.
- Ids: dataset/world/agent/label keys, exchange ids (ULIDs, derived bit for
  bit as crosstalk-eval derives them), message ids, digests.
- JSONL framing: header, per-world sections, trailer with counts and digest.
- Checks across one world's files (`check`).

## Non-scope

- Converters, scoring, gates, the reference matcher, the CLI.
- Resource canonicalisation: `Resource` is a shape here;
  `a2a-bench-resource` canonicalises.
- Dataset bytes: tests use a synthetic world (`tests/common`).

## Files of an export and a run

```text
<export>/manifest.json      Manifest                     what the export holds; digests of the files
<export>/messages.jsonl     Messages     world: {key}               rows: message
<export>/exchanges.jsonl    Exchanges    world: {key, agents}       rows: exchange
<export>/labels.jsonl       Labels       world: {key, coverage}     rows: exchange_agent, transmission, access_only,
                                                                          negative_control, exemption, agent_cluster
<run>/predictions.jsonl     Predictions  world: {key, status}       rows: attribution, unattributed, transmission
```

A detector gets the **input view**: the manifest without the labels digest
(`Manifest::input_view`), `messages.jsonl` and `exchanges.jsonl`.

### Framing (`jsonl`)

Every JSONL file is:

```text
{"kind":"header","format":"a2a-bench/1","file":"<name>","dataset":"<id>", …}
{"kind":"world","key":"<world>", …}          one section per world …
{"kind":"<row kind>", …}                      … holding its rows
{"kind":"trailer","worlds":N,"rows":M,"digest":"<hex>"}
```

- Worlds appear in the same order in every file of an export, so a reader
  holds one world at a time (`FileReader::next_world`), and memory is
  bounded by the largest world, never the dataset.
- Messages are de-duplicated **within a world** only.
- The trailer's digest is the BLAKE3 (derive-key context
  `a2a-bench/1 file`) of every line before it, newline included. A file
  without a trailer is `Truncated`; a dropped, swapped or added line fails
  `Counts`, `Digest` or `AfterTrailer`.
- The predictions header also carries `detector {name, version, variant,
  config_digest?}` and `manifest_digest`.

### Rows

- **message** `{id, body: {role, parts}}`. Part enums per role mirror
  crosstalk-spec's, so a part's index is the same on both sides:
  system `text | unknown`; user `text | media | unknown`; assistant
  `text | reasoning | reasoning_opaque | tool_call | server_tool_result |
  unknown`; tool `tool_result` (one or more). Tool results are never user
  parts. Signatures, encrypted reasoning and media bytes are not stored.
  `id` is the keyed BLAKE3 (`a2a-bench/1 message`) of the body's canonical
  JSON; a stored id that is not the body's is refused.
- **exchange** `{id, at_us, client, request {messages, tools?}, response
  {messages, stop?}, fidelity, source}`. `request.messages` is one ordered
  list with system messages inline. `client` is what a proxy observes
  (credential fingerprint, session, turn, vendor, model), never the agent.
- **labels**: as crosstalk-eval's truth, plus an `id` per row and the
  `exchange_agent` rows that say which agent made each exchange.
- **predictions**: `attribution {agent, exchanges}` (the detector's agents,
  canonical names), `unattributed {agent}`, and `transmission {id, state,
  quality?, matches?, co_access?}`. A content match's `route` is a
  `PredictedRoute`: a channel names **every** resource the detector's channel
  holds, and a channel label aligns when one of them equals the label's
  resource after canonicalisation.
  Evidence order is normative: a transmission's `matches` are sorted by
  `read_at` and its `co_access` by `(read_at, write_at)`, non-decreasing in
  `Location`'s order (exchange, message, part, range); `check_predictions`
  refuses anything else as `PredictionError::Unsorted`. Order is semantic,
  not cosmetic: the quality row takes the carrier of the first match of
  the strongest class, so two files holding the same matches in another
  order could report another carrier.
- **resources**: `repository`, `repo_file` (absolute path), `thread`
  (`issue` covers GitHub issues and pull requests, one number space;
  `merge_request`), `collection`, `url`, `file {host?, path}`,
  `mcp {server, tool, target?}`, `opaque {tool, key}`.
- **media** parts keep only their kind (`image`, `audio`, `document`,
  `other`). A failed exchange has `response.error` (and may have no
  messages).
- **agent_cluster** rows carry their kind in `cluster` (`key_group`,
  `identity`), since `kind` is the row tag. A key group of one agent is not
  a cluster: converters skip it and count it in `notes`.

### Locations: anchors, not scopes

A location's `exchange` is where the location **resolves**: an exchange
that carries `message` (request or response). It is not a scope. Alignment
and control coverage compare message, part and range
(`Location::overlaps`), and a row's reader exchange, when it has one, is
what limits it to an exchange. Normative consequences:

- A transmission label's, exemption's and prediction's read location sits
  in its reader exchange; a co-access's write location in its write exchange.
- A negative control that names a message but no exchange (a shared
  system prompt, boilerplate, a rejected send's origin) is **placed** at the
  first exchange, in world time order, of the reader (for an `origin`: of
  the sender) that carries the message, else at the world's first exchange
  that carries it; its `reader_exchange` stays absent, so it still covers
  every exchange carrying that text. crosstalk-eval's golden export
  (`golden/labels.rs`) uses the same rule.
- A label whose message no exchange in the world carries can never match
  anything. Converters **drop** it and count it in the manifest's per-world
  `notes` (`uncarried_control`, …), which the scorer reports; a world is
  never failed for it.

### Locations and part text

A location is `{exchange, message, part, range {start, end}}`, a non-empty
byte range of the part's text:

| Part | Text |
| --- | --- |
| `text` (any role), `reasoning` | the text |
| `tool_call` | `arguments.json` (canonical JSON) or `arguments.invalid` (verbatim) |
| `tool_result`, `server_tool_result` | its text contents in order, joined with `"\n"` |
| `reasoning_opaque`, `media`, `unknown`, a result without text | none |

## Data and control flow

```text
converter ──▶ Message::new / Exchange / Label constructors (row checks)
          ──▶ FileWriter<K>::new(header) · world(..) · row(..) · finish() ──▶ trailer digest
          ──▶ Manifest { files: trailers' digests }

reader    ──▶ FileReader<K>::open (header: file kind, format)
          ──▶ next_world() per file, in lockstep ──▶ WorldInputs::new (inputs checks)
          ──▶ check_labels(inputs, labels) / check_predictions(inputs, predictions)
          ──▶ trailer checked when the last world is read
```

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `src/lib.rs` | crate root | modules |
| `src/version.rs` | the format version | `FORMAT`, `Format` |
| `src/json/` (`mod`, `parse`, `number`, `write`) | exact-number JSON and its canonical text (ported from crosstalk-spec) | `Json`, `CanonicalJson`, `Number`, `JsonError`, `CanonicalJsonError` |
| `src/ids/mod.rs` | keys, exchange and message ids, digests | `DatasetId`, `WorldKey`, `AgentKey`, `LabelId`, `DetectorAgent`, `TransmissionRef`, `SourceRef`, `ExchangeId`, `MessageId`, `Digest`, `InvalidKey` |
| `src/ids/ulid.rs` | Crockford base32 text | `InvalidUlid`, `ULID_LEN` |
| `src/ids/derive.rs` | exchange id derivation | `exchange_id`, `DOMAIN` |
| `src/time.rs` | microsecond timestamps | `Timestamp` |
| `src/message/` (`mod`, `part`, `text`) | messages, parts per role, part text | `Message`, `Body`, `SystemPart`, `UserPart`, `AssistantPart`, `ToolPart`, `ToolCall`, `ToolResult`, `ToolArguments`, `NoPartText`, `MESSAGE_ID_CONTEXT` |
| `src/exchange.rs` | worlds and exchanges | `WorldDecl`, `AgentDecl`, `Driven`, `Exchange`, `Client`, `Request`, `Response`, `ToolDecl`, `Fidelity`, `Side` |
| `src/location.rs` | locations | `Location`, `ByteRange`, `EmptyRange` |
| `src/resource.rs` | resource shapes | `Resource`, `Repository`, `ThreadKind`, `CollectionKind` |
| `src/labels/` (`mod`, `kinds`) | truth rows and their dimensions | `Label`, `ExpectedTransmission`, `ExpectedAccess`, `NegativeControl`, `Exemption`, `AgentCluster`, `ExchangeAgent`, `Tier`, `MatchNeed` (with `json_string`, `yaml_string`, `through_json_string`, `two_string_levels`, `sender_medium_unobserved`, `tier`), `json_escapes`, `Route`, `CarrierKind`, `Codec`, `InvalidLabel` |
| `src/predictions.rs` | detector rows | `Prediction`, `PredictedRoute`, `Transmission`, `State`, `Quality`, `ContentEvidence`, `CoAccess`, `MatchKind`, `Attribution`, `Unattributed`, `WorldStatus`, `InvalidTransmission` |
| `src/files.rs` | the four file kinds | `Messages`, `Exchanges`, `Labels`, `Predictions`, their world rows, `Coverage`, `PredictionsHeader`, `DetectorInfo` |
| `src/manifest.rs` | the manifest | `Manifest`, `Split`, `Source`, `Converter`, `Setting`, `WorldEntry`, `FileDigests` |
| `src/source.rs` | the source digest | `SourceDigest`, `FileDigest`, `SOURCE_DIGEST_CONTEXT`, `SourceDigestError` |
| `src/jsonl/` (`mod`, `read`, `write`) | framing (`FileReader::trailer` after the last world) | `FileKind`, `FileReader`, `FileWriter`, `WorldSection`, `BasicHeader`, `Trailer`, `ReadError`, `WriteError` |
| `src/check/` (`mod`, `inputs`, `location`, `labels`, `predictions`) | cross-file checks | `WorldInputs`, `check_labels`, `check_predictions`, `InputError`, `LabelError`, `PredictionError`, `LocationError` |
| `tests/fixtures/crosstalk-7f8a2fb-vectors.json` | exchange ids and canonical JSON computed by crosstalk at 7f8a2fb | |
| `tests/fixtures/crosstalk-vectors-generator.rs.txt` | the program that computed them (built against crosstalk-spec and crosstalk-eval) | |

## Manifest

`manifest.json` holds the format, dataset, `dataset_version`, `split`,
`source {path, revision, digest}`, `converter {version, git}`,
`selection` and `pace` (ordered maps of `Setting`: bool, int, text, or a
list of them), `worlds` (`{key, exchanges, labels?, notes?}`) and `files`
(each file's trailer digest; `labels` absent in the input view). The input
view also drops every world's `labels` count and `notes`, which are
truth-derived. `Manifest::digest` is over the input view, so the full
manifest and the input view name the same digest.

### Source digest

`source.digest` is BLAKE3, derive-key context `a2a-bench/1 source`, over
the files the converter read, in byte order of their `/`-separated paths
relative to the dataset root, each absorbed as
`path 0x00 length(u64 little-endian) contents` (`source::SourceDigest`).
It identifies exactly what was read; the dataset's own revision is
`source.revision`.

## Invariants and constraints

- **Parity with crosstalk.** Exchange ids, canonical JSON and part text
  equal crosstalk's (tested by vectors crosstalk computed; part text is
  checked again on every golden export in the parity stage P1). The id
  derivation's domain string stays `crosstalk-eval/v1` for that reason.
- **Checked types.** A row that exists passed its checks: a message's id is
  its body's; labels have distinct sender and reader (except `self_read`
  controls), content as long as its location and in the reader exchange,
  a need out of reach exactly when the tier is; controls are bounded;
  exemptions sit in their reader exchange; clusters hold two or more
  distinct agents; transmissions carry exactly the evidence their state
  allows; ranges are non-empty; `CanonicalJson` is canonical.
- **Strict reading.** Unknown fields and kinds are refused; a reader
  accepts exactly its format major.
- **Cross-file checks** (`check`): every message once per world and used;
  exchanges once each and in time order; each exchange assigned to exactly
  one declared model-driven agent, each agent's exchanges strictly
  increasing; locations resolve on character boundaries; label text equals
  the text at its location; predictions attribute each exchange at most
  once, name only attributed or declared-unattributed agents, and store
  each transmission's matches sorted by `read_at` and co-accesses by
  `(read_at, write_at)`.
- **Determinism.** Writing the same rows gives the same bytes; every map
  that reaches output is ordered.
- **Versioning.** Changing part text, canonical JSON, id derivation, a
  digest context or a row's fields is a new format major.
