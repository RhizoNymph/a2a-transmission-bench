# swarm-traces converter (`a2a-bench-dataset-swarm`)

A decoder test corpus (`crates/datasets/swarm`). Dataset id
`swarm-traces`, version 1 (`swarm-traces@1`): crosstalk-eval's converter
(`src/datasets/swarm/`) at the pinned commit 7f8a2fb, ported onto the
bench's format and corpus types for parity.

**The export holds real attack payloads.** The converter treats them
purely as text: it never executes anything in them and never fetches any
URL in them. Nothing the crate writes to a log, an error or a tally
carries payload text (errors name a token by its length and a malformed
line by its number, column and JSON error category; the tally holds
chains, counts and byte lengths). No dataset bytes are committed; the
fixtures are benign synthetic strings that mimic the encoding structure.
A label's text is the encoded token itself, so exported labels must be
handled like the dataset (the CLI must not print them, as ct-eval's
`truth` refused to and its `run` forced `--examples 0`).

## Scope

- Reading `redacted.jsonl[.gz]` (gzipped preferred), rows
  `{id, kind ∈ {payload, recovered_text, response}, parent_id, text}`,
  recording the file read.
- Extracting candidate tokens per payload, decoding each through a chain
  of codecs, and planning one two-agent world per token that decodes to
  printable text through a chain of format codecs (`Options::limit` caps
  the worlds, as ct-eval's `--limit`).
- The decode-chain tally (`ChainTally`).

## Non-scope

- Byte-escape (`\x..`) chains: no format codec names them; they are
  counted as gaps, never converted.
- Writing, printing or reporting payload text anywhere.
- The CLI, the manifest, detector runs.

## Data and control flow

```text
source(root, &Options, pace) ─▶ SwarmSource::open
  read::rows ─▶ Vec<Row>, file order (invalid UTF-8 replaced)
  corroborated = parent ids of recovered_text / response rows
  for each payload row (tally.payloads += 1):
    tokens::tokens(text), each ≥ 16 bytes, in this order:
      atob('…') / atob("…") inner strings;
      runs of [A-Za-z0-9+/=_-]; runs of hex digits;
      runs of non-whitespace, non-'"' ASCII; runs of \xNN escapes
    for each (index, token), first occurrence only (tally.candidates += 1):
      codec::decode(token): peel layers (byte escape, then URL, then hex,
        then base64; each must leave ≥ 90 % printable UTF-8), at most 6
      plaintext ≥ 24 bytes and ≥ 20 word chars ─▶ tally.add(chain, lengths, corroborated)
      every layer a format codec ─▶ TokenWorld {payload id, index, token, corroborated}
      stop all reading once `limit` worlds are planned

TraceSource::select(filter) ─▶ worlds not kept are never built
TraceSource::worlds() ─▶ build::world per TokenWorld, key "<payload id>#<index>"
  agents "author", "reader" (model "swarm/agent")
  call 0  author: [system] ─▶ decoded plaintext            source /row/<id>/plaintext
  call 1  reader: [system, user "Fetch the drop."] ─▶ fetch_drop {}   /row/<id>/token/<i>/call
  call 2  reader: [system, user, call, result(token)] ─▶ "Fetched the drop."   /row/<id>/token/<i>
  label: transmission author → reader, direct, tool_result, the whole token
         in the result (bytes 0..len), needs decoded{chain}, no sender
         exchange, tier construction if corroborated else structural,
         id = source = /row/<id>/token/<i>
  finish(Coverage::Complete { tier: structural })
```

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `src/lib.rs` | constants, options, the source and its planning, `TraceSource` | `DATASET`, `VERSION`, `PAYLOADS_FILE`, `Options` (`settings`), `source`, `SwarmSource` (`open`, `tally`, `world_count`, `files_read`) |
| `src/error.rs` | the converter's errors, none holding payload text | `SwarmError`, `category` |
| `src/read.rs` | the export's rows | `Row`, `rows` |
| `src/tokens.rs` | candidate tokens of a payload | `tokens` |
| `src/codec.rs` | the nested codec-chain decoder | `decode`, `Layer`, `Decoded` (`codecs`, `chain_name`) |
| `src/build.rs` | one token's two-agent world | `world`, `TokenWorld` (`key`), `MODEL` |
| `src/messages.rs` | the synthetic messages | (crate-private) |
| `src/tally.rs` | decode-chain counts and lengths | `ChainTally` (`add`, `labelled_tokens`), `ChainStats` |
| `tests/swarm.rs` | decoding, tokens, worlds, tiers, chains, tally (no text leaks), the harness shape, limit and selection, determinism, errors without text | |
| `tests/fixtures/swarm-traces/redacted.jsonl` | four benign synthetic payloads (ct-eval's fixture) | |

## Invariants and constraints

- **Parity (version 1).** Same worlds in the same order, same exchanges
  (ids, times, messages part for part) and same labels as ct-eval at
  7f8a2fb on the same selection and pace (see below). Any change that
  alters an exported byte is `swarm-traces@2`.
- **No payload text leaves the converter** except inside the world (the
  messages and the label text an export holds). Not in logs, errors or
  the tally.
- A token is counted once per payload; its index is its position in the
  candidate list before de-duplication, so world keys are unique.
- Only chains whose every layer is base64, hex or URL-encoding become
  worlds, so every label's `needs` is a reachable `decoded` chain; tier is
  never out of reach.
- Coverage is complete at Structural: the token is the only cross-agent
  content in a world.
- The three calls are pace steps 0, 1 and 2, so they are 1 to 5 s apart
  under the default pace.

## Known ct-eval quirks kept

- Duplicate token texts across the extractors are skipped after the
  first, but the world key keeps the first occurrence's index, so indices
  have gaps.
- The decoder trims whitespace between layers and accepts any of four
  base64 alphabets/paddings; URL decoding turns `+` into a space.
- `fetch_drop`'s call id is `fetch-<payload id>`, shared by every world of
  a payload (worlds are separate, so ids never collide within one).
- The reader's label has no sender exchange although the author's
  exchange originates the plaintext (ct-eval left it `None`).
- The tally counts tokens of every chain that reached the length floor,
  including the byte-escape gaps, before the limit stops reading.

## Differences from ct-eval

- **Errors** hold no payload text: a malformed line reports its number,
  column and JSON error category, not serde's message (which can quote a
  value). Label ids are new (ct-eval had none): the source path.
- **`atob(` scanning** steps over the next character whole, where ct-eval
  stepped one byte (and would panic on a multi-byte character there);
  identical wherever ct-eval does not panic.
- **Tests.** ct-eval's reference-matcher test keeps its label-side
  assertions (three decoded labels, two single-layer); finding them is the
  reference workstream's.

## Real-data comparison

Run on 2026-10-05 against `~/Data/ai/agents/swarm-traces`
(`redacted.jsonl.gz`), in full, default pace. ct-eval's `truth` refuses
this dataset, so the ct-eval side is its `run --detector reference
--examples 0` report totals and a probe program built outside the
repository against crosstalk-eval at 7f8a2fb, which dumps world keys,
agents, exchanges (id, time, agent, and a BLAKE3 of each message's role
and part texts) and labels with every text replaced by its BLAKE3; the
bench side dumps the same from this crate. Nothing was printed but
counts.

| Chain | Tier | Labels (ct-eval / bench) |
| --- | --- | ---: |
| `url` | construction | 168 / 168 |
| `url` | structural | 447 / 447 |
| `url.url` | construction | 2 / 2 |
| `url.url` | structural | 11 / 11 |
| `base64` | structural | 1 / 1 |
| **all** (direct, `tool_result`, `decoded`) | | **629 / 629** |

| | ct-eval | bench |
| --- | ---: | ---: |
| Worlds | 629 | 629 |
| Agents | 1,258 | 1,258 |
| Exchanges | 1,887 | 1,887 |
| Failed worlds | 0 | 0 |

World keys are equal in order; the (world, exchange id, time, agent) list
is equal in order; every exchange's messages are equal part for part;
labels are equal in order in every field but the message id inside
locations (spec `MessageHash` vs bench `MessageId`). No byte-escape chain
reaches the floor on this export, so no gap is tallied.
