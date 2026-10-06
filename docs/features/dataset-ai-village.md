# AI Village converter (`a2a-bench-dataset-ai-village`)

[AI Village](https://theaidigest.org/village) (AI Digest) is a long-running
experiment in which frontier-model agents share a village: each agent has
its own computer, a group chat, memories and goals. The dataset is a
near-verbatim dump of the village database as gzipped JSON Lines under
`~/Data/ai/agents/ai-village` (`ai-village` in `datasets.toml`; read
`SCHEMA.md` and `CHANGELOG.md` there). The crate
`crates/datasets/ai-village` (package `a2a-bench-dataset-ai-village`,
dataset id `ai-village`, version `ai-village@1`) turns it into bench
worlds, in two modes.

Version 1 is a port of crosstalk-eval's converter at crosstalk `7f8a2fb`
(`crates/eval/src/datasets/ai_village`), **without its dependency on
crosstalk's extractor**: ct-eval decided each bash command's resource
accesses by running crosstalk-flow's `ToolExtractors` (design §5.2), so
its labels matched the detector by construction. The bench decides them
itself with its own command table and shell model ([`shell`](#the-shell-model-normative)),
whose resource identity is `a2a-bench-resource`'s. `@1` reproduces
ct-eval's labels exactly; the [agreement check](#agreement-with-crosstalk-at-7f8a2fb)
proves it command by command.

## Scope

- **Claude Code mode** (`mode = claude-code`): the one agent that ran the
  Claude Agent SDK ("Opus 4.5 (Claude Code)", 2026-01-26..03-31), one
  world per context (a session cut at compaction boundaries). Its SDK
  entries give exact call boundaries and the tool results it read.
  Construction-tier labels for every chat message the agent read through
  the village MCP server's `get_events` tool, keyed by event id.
- **Window mode** (`mode = window`, `from`..=`to`, default
  2026-07-13..07-17): every standard agent over a window of village days,
  one world per day, with rebuilt requests, structural chat labels,
  heuristic content labels on repository files, threads and pages, and
  access-only labels on repositories; GUI edits of Google Docs and Gmail
  counted. `hours = N` (with `from == to`) keeps only the first N hours of
  the day from its 10:00 UTC start.
- **The shell model** (`shell`): a bash command and its recorded output →
  the shared-resource reads and writes it made, with each write's outcome
  and the text its author typed. The bench's own definition, normative
  below.
- `Options` (ct-eval's `--mode`, `--from`, `--to`, `--hours`, `--limit`
  with its defaults), `Options::settings` for the manifest's selection,
  `source(root, &options)`, and the tables read (`files_read`) for the
  source digest.

## Non-scope

- **Screenshots** (`images/`) are never read; image blocks are dropped.
- **Exact requests.** `llm_calls` is withheld: standard agents' requests
  are rebuilt from the responses and the Claude Code agent's system prompt
  and per-query prompts are missing.
- **The Claude Code agent in window mode** (its calls are not in
  `computer_use_turns`).
- **Co-access labels off the repository**, **local files**, **GUI
  transmissions**: as ct-eval (counted, never labelled).
- **ct-eval's `report.rs`** (`Unlabelled`, the unlabelled-prediction
  breakdown): it reads predictions and the alignment rule, so it belongs to
  the scorer or CLI, not the converter.
- **Token usage**: the bench's exchanges carry none, so the SDK's usage
  counts are not read.
- **The CLI wiring** (another workstream).

## Data and control flow

```text
ai-village/*.jsonl.gz ──▶ stream::Table::scan (flate2 MultiGzDecoder, line by line; created_at read from the raw line first)
   │
   ├─ claude-code ─▶ ClaudeCodeStream::open
   │                   entries::load (all rows, ordered by session, time, row id)
   │                   entries::contexts (session cut at compact_boundary)
   │                   delivered_events → tables::events_by_id (one events pass, ids only)
   │                   chat_messages pass (the agent's own messages)
   │                 next_world (one per context):
   │                   calls::context → Calls (requests) + ResultRefs (+ the call that first carries each)
   │                   exchanges (reconstructed) ─┐
   │                   get_events results ─▶ events::talks (escaped content located)
   │                     first delivery per event id ─▶ originating exchange (event data.output, synthetic)
   │                                                 ─▶ label direct / tool_result / construction
   │                   chat_message calls ─▶ matched to chat_messages (stats)
   │
   └─ window ─────▶ WindowStream::open
                       tables::{load_directory, load_sessions, load_goals}
                       tables::scan_turns (raw lines bucketed by village day)
                       tables::scan_events (AGENT_TALK, USER_TALK; RoomTimeline from every event)
                       tables::scan_chat, tables::scan_memories
                       tag: per day, turns sorted by (agent, time, id); every bash turn ─▶
                            shell::Shell::accesses (one shell per agent across the window)
                            ─▶ window::repo::AccessLog (pairs: content / access only);
                            GUI turns ─▶ gui::GuiStats
                     next_world (one per village day): window::day::build
                       calls::turn_call per turn (provider::response), talk_call for an unmatched AGENT_TALK
                       requests: system prompt + session history + chat user turn (prompt)
                       chat labels (structural), repository content and access-only labels (heuristic)
                                     │
                                     ▼
                  WorldBuilder::finish (format checks) ─▶ World ─▶ TraceSource::worlds
```

### Tables, time, requests, labels

Unchanged from ct-eval (its `eval_ai_village.md`), restated here:

- Every table is sorted by UUID, so a window needs a full pass; rows
  outside it are dropped from the raw line's `created_at` before decoding.
  Times are UTC text parsed to microseconds. A **village day** runs
  10:00–10:00 UTC.
- **Claude Code requests.** A call (every assistant entry with one
  `message.id`) gets the context's history when its first block arrived;
  results that arrive between a message's blocks are placed after it.
  `reconstructed`. `get_events` results are pretty-printed JSON; each
  `AGENT_TALK` content is located at its JSON-escaped bytes (the last
  occurrence before its `"id"`). The first result that delivers an event
  id to a call is a label author → Claude Code, `direct`, `tool_result`,
  at the first call carrying the result. The author's exchange is the
  event's `data.output` at the event's time with an empty request
  (`synthetic`).
- **Match needs** (`text::need`): `exact`; `decoded [json_string]` when one
  JSON unescape gives a verbatim match; `normalized` (case and whitespace
  folding, `fold::fold_plain`); `decoded [json_string]` with one string
  level undone on either side then folded; out of reach
  (`undecodable json_string+json_string`, tier `out_of_reach`) with exactly
  two levels; `decoded [json_string]` when only the matching fold
  (`fold::fold`) matches; else `semantic`.
- **Window requests** (assumed placement):

  ```text
  system: "You are <name>…" + village goal + the agent's goals + its latest memory   (per session)
  for each earlier call of the same computer-use session:
      user: that call's chat turn (if any)
      assistant: that call's response
      tool: its output, as the result of its first tool call
  user: chat posted in the agent's room since its previous call,
        one "[YYYY-MM-DD HH:MM:SS UTC] #room speaker: content" line each
  ```

- **Rooms**: everyone in `#general` before 2026-02-25; after, the room of
  the agent's latest event naming one (`ENTER_ROOM` records the previous
  room just before the move).
- **Chat labels** (structural): an `AGENT_TALK` reaches every other room
  member through the chat turn of their next call within four hours, from
  the speaker's sending call; `direct`, `user_turn`.
- **Pairs** (`window::repo`): a read whose latest earlier non-rejected
  write to the same shell locator came from another agent.

  | Write | Pair | Label |
  | --- | --- | --- |
  | `git push` (unseen payload) | access only | `access_only`: `channel {repository}`, `tool_result`, heuristic, over the read's whole output |
  | authored text, a line of which (≥ 24 bytes, ≥ 20 letters or digits) is in the read's output | content | `transmission`: `channel {resource}`, `tool_result`, heuristic, at that line, `needs` from the writer's call |
  | authored text not in the output | co-access only | counted (`repo_co_access`) |

  Both sit at the reader's next call in the same session. A pair whose
  write is on an earlier day is counted, not labelled.

### Bench shapes

- Messages are bench messages (`a2a_bench_format::message`): signatures
  are dropped (signed thinking is `reasoning`), encrypted reasoning is
  `reasoning_opaque`, unknown blocks are `unknown` with no bytes. Part
  boundaries and order are ct-eval's, so part indices and part text are
  the same.
- A channel label's resource is `Loc::resource` of the shell locator: a
  forge repository, a repository file, a GitHub/GitLab thread or
  collection (a URL that is exactly such a page), else the URL.
- Label ids are `<world>/<n>`, numbered in the order the converter adds a
  world's labels (ct-eval's labels had none).
- Exchange ids derive from `(ai-village, source ref, time)` exactly as
  ct-eval's. Claude Code exchanges name the call's model; window exchanges
  the agent's.

## The shell model (normative)

The bench's definition of what a village bash command did to shared
resources, from the command and its recorded output (the village records
no exit status). `ai-village@1` keeps every behaviour crosstalk's L5
extractor had at 7f8a2fb as ct-eval drove it, including the quirks listed
under [@2](#ai-village2-changes-decided-not-made).

### Shell state

| State | Rule |
| --- | --- |
| Shells | one persistent shell per agent (window: across the whole window; the agreement check: one per Claude Code session) |
| Start | working directory `/home/computeruse`, no clones known |
| `~`, `$HOME`, `${HOME}` | replaced by `/home/computeruse` before the command is read (`~` only at a word's start, before `/` or the word's end) |
| Lexing | quotes and escapes resolved; `$…`, backticks, globs and a leading `~` make a word non-literal, which names nothing; here-document bodies skipped; a command the lexer refuses (unterminated quote or substitution, a redirection with no target) has no access and teaches nothing |
| Sequencing | simple commands split at `;`, `&`, `&&`, `\|\|`, `\|`, newlines, parentheses and run in order; a failed command does not stop a chain |
| Program | past variable assignments, `{`/`}` and the wrappers `sudo`, `env`, `command`, `builtin`, `exec`, `nohup`, `time`, `timeout` (and their value options) |
| `cd <dir>` | moves to `<dir>` (absolute, or joined lexically to the current directory) whether or not it exists; `cd` with no target, `cd -`, `cd ~…` (after expansion, only a literal `~` left by quoting) or an expansion makes the directory unknown; a relative path from an unknown directory stays unknown |
| Relative paths | resolved against the working directory; with none known, the path is keyed as written (`opaque`, never shared) |
| Clones | `git clone <remote> [<dir>]`, `gh`/`glab repo clone <repo> [<dir>]`: `<dir>` (else the remote's last segment without `.git`, in the working directory) is bound to the repository; `git remote add\|set-url <name> <url>` binds the working directory; a later binding of a directory replaces the earlier one; a path is in the clone whose root is its longest ancestor |
| Remote queries | a command that ran exactly one `git remote -v\|--verbose\|get-url` or `git config [--get] remote.<n>.url` (at most three arguments, no `--add`, `--unset`, `--replace-all`) in a known directory binds that directory to the first remote its output shows (a word with `://`, `:` or a leading `/` that parses as a remote) |
| Printed remotes | after a command, if the shell's directory is in no clone and the command contains `git` and `push` (`To <remote>`) or `git` and `pull`/`fetch` (`From <remote>`) and the output has such a line naming a forge remote, the directory is bound to it, and the command's accesses are taken again from the shell as it was before the command plus that binding |
| End directory | the directory the last command left; a line `Shell cwd was reset to <path>` sets it |

### Command table

| Command | Access | Locator | Judged by |
| --- | --- | --- | --- |
| `> f`, `>> f`, `>\| f`, `&> f`, `1> f`, `>& f` (not a descriptor) | write | the file | delivered |
| `cat`, `head`, `tail`, `tac`, `bat` `f…` (or `< f`), stdout not to a file | read | each file | delivered |
| `sed -n '<N>p'`, `'<N>,<M>p'`, `'$p'` `f…` (no `-i`), stdout not to a file | read | each file | delivered |
| `tee f…` | write | each file | delivered |
| `git clone <remote>` | read | the repository | git transfer |
| `git push [<remote>\|--repo r]` (not `-n`/`--dry-run`) | write, payload unseen | the remote operand's repository (a URL or path), else the bound clone's | git push |
| `git pull`, `git fetch [<remote>]` (not `--dry-run`; `--all` and `--multiple` ignore the operand) | read | as push | git transfer |
| `git show`, `git cat-file` `<rev>:<path>`, stdout not to a file | read | the clone's file at `/<path>` (`./`, `../` from the working directory) | delivered |
| `git -C <dir> …` | | as the subcommand, run in `<dir>` | |
| `gh`/`glab repo clone <repo>` | read | the repository (`owner/name`, `host/owner/name`, `group/sub/name`, a URL or remote) | git transfer |
| `gh issue\|pr`, `glab issue\|mr` `create` | write | the collection: GitHub `/issues`, `/pulls`; GitLab `/-/issues`, `/-/merge_requests` | forge CLI |
| … `comment`, `note`, `edit`, `update`, `review` | write | the thread of the number or URL operand (GitHub `/issues/<N>` for issues and pulls alike; GitLab `/-/issues/<N>`, `/-/merge_requests/<N>`), else the collection | forge CLI |
| … `view` | read | the thread, else the collection | forge CLI |
| … `list` | read | the collection | forge CLI |
| (repository of the above) | | `-R`/`--repo`, else the bound clone's; none: no access | |
| `gh api`, `glab api <endpoint>` (not `graphql`) | the method's | `https://api.github.com/<endpoint>` (`https://<host>/api/v3/…` with `--hostname`), `https://gitlab.com/api/v4/<endpoint>`, placeholders from the bound clone; method `-X`, else `POST` with `-f`/`-F`/`--field`/`--raw-field`/`--input`, else `GET` | forge CLI |
| `curl` | the method's | each URL operand and `--url` (a bare host is `http://`); method `-X`, else `HEAD` for `-I`, else `POST` with a body (`-d`, `--data*`, `--json`, `-F`, `--form*`, `-T`) unless `-G`, else `GET`; `-o f` writes `f` | HTTP |
| `wget` | the method's | each URL operand; `--method`, else `POST` with `--post-data`/`--body-data`/`--post-file`/`--body-file`; the body reaches the output only with `-O -`; `-O f` writes `f` | HTTP |

An HTTP request's accesses: the site rule's when one recognizes it (below),
else a writing method (`POST PUT PATCH DELETE`) writes the URL, a reading
one (`GET HEAD`) reads it when the body reaches the output, any other
method is no access.

| Site | `GET`/`HEAD` | writing method | other method |
| --- | --- | --- | --- |
| GitHub/GitLab API (`api.github.com/repos/o/n/…`, `gitlab.com/api/v4/projects/<path>/…`) | read of `canonical_url`'s repository, file, thread or collection | write of it | none |
| GitHub `blob`/`raw` pages, `raw.githubusercontent.com` | read of the file | read of the file | read of the file |
| other forge pages (web, `tree`, `codeload`, Pages) | read of `canonical_url`'s resource | no site rule | no site rule |
| MediaWiki (`*.wikipedia.org` … `*.fandom.com`) | the page per `shell::http::mediawiki` (query and form fields are the parameters; `api.php` edits, `index.php?action=submit` write) | | |

URLs are `a2a_bench_resource::normalized_url`; a `GET`'s site resource is
`a2a_bench_resource::canonical_url`; remotes and repository parts are
`canonical_remote` and `repository`.

### Outcomes and kept accesses

| Judged by | `rejected` | `delivered` | otherwise |
| --- | --- | --- | --- |
| git push | a line opening `! [rejected]`, `! [remote rejected]`, `error:`, `fatal:`, `remote: Permission`, `remote: Invalid`, `Permission denied` | a ref update (`a..b  x -> y`, `* [new …]`), `Everything up-to-date` | unknown |
| git transfer | a line opening `fatal:` or `error:` | | delivered |
| HTTP | a `curl: (N)` line; the last status shown (`HTTP/x N`, wget's `… N …`, `ERROR N:`, the code a `-w '%{http_code}'` printed last) ≥ 400 | the last status shown < 400 | delivered |
| forge CLI | a line opening `gh: `, `glab: `, `GraphQL:`, `error:`, `ERROR:`, `HTTP 4`, `HTTP 5`, `could not`, `failed to`, `X `, or ending `(HTTP ≥400)` | an `https://` or `✓` line | delivered |
| delivered | | always | |

The rules read the whole output of the call. A read judged `rejected` is
dropped. An access identical to an earlier one of the same command is
dropped. Only shared locators are kept: a repository, a file of a forge
repository, a URL (not a local file, a file of a local repository, or an
opaque key). A write's payload is unseen for `git push`, else the
authored texts of the command (`shell::payload::authored`: here-document
bodies, `--body -b --title -t --description -d --message -m --data*
--json --form* --post-data --body-data` values, `-f -F --field
--raw-field` values after `=`, `echo`/`printf` arguments).

### Spec table

Every row is a test (`tests/spec.rs` reads this table): the command runs in
a fresh shell with the given output (`\n` is a line break, `\|` a pipe);
accesses are `read <locator>` or `write <outcome> <unseen|authored>
<locator>`, separated by `; `, or `none`; locators are written
`repo://h/o/n`, `file://<host><path>` or the URL.

<!-- spec-table:start -->
| Command | Output | Accesses | Directory after |
| --- | --- | --- | --- |
| `git push` | | none | `/home/computeruse` |
| `cd /home/computeruse/r && git push` | `To https://github.com/o/r.git\n   1a2b3c4..5d6e7f8  main -> main` | write delivered unseen repo://github.com/o/r | `/home/computeruse/r` |
| `git clone https://github.com/O/R.git && cd R && git pull` | | read repo://github.com/o/r | `/home/computeruse/R` |
| `git clone git@gitlab.com:g/s/p.git x && cat x/README.md` | | read repo://gitlab.com/g/s/p; read file://gitlab.com/g/s/p/README.md | `/home/computeruse` |
| `git clone https://github.com/o/r && cd r && git push` | `! [rejected]        main -> main` | read repo://github.com/o/r; write rejected unseen repo://github.com/o/r | `/home/computeruse/r` |
| `git clone https://github.com/o/r && git -C r fetch` | `fatal: unable` | none | `/home/computeruse` |
| `git push --dry-run https://github.com/o/r` | | none | `/home/computeruse` |
| `git push https://github.com/o/r main` | `Everything up-to-date` | write delivered unseen repo://github.com/o/r | `/home/computeruse` |
| `git push https://github.com/o/r main` | `nothing` | write unknown unseen repo://github.com/o/r | `/home/computeruse` |
| `git remote add origin https://github.com/o/r && git pull` | | read repo://github.com/o/r | `/home/computeruse` |
| `git clone https://github.com/o/r && cd r && git show HEAD:src/a.py` | | read repo://github.com/o/r; read file://github.com/o/r/src/a.py | `/home/computeruse/r` |
| `gh issue create -R o/r --title 'Hello there'` | `https://github.com/o/r/issues/4` | write delivered authored https://github.com/o/r/issues | `/home/computeruse` |
| `gh pr comment 7 -R o/r --body 'Looks good'` | `https://github.com/o/r/pull/7#c` | write delivered authored https://github.com/o/r/issues/7 | `/home/computeruse` |
| `gh pr view https://github.com/o/r/pull/7` | `x` | read https://github.com/o/r/issues/7 | `/home/computeruse` |
| `gh issue list -R o/r` | `x` | read https://github.com/o/r/issues | `/home/computeruse` |
| `glab mr note 3 -R g/s/p -m 'ok'` | `x` | write delivered authored https://gitlab.com/g/s/p/-/merge_requests/3 | `/home/computeruse` |
| `glab issue create -R g/p -t 'T'` | `x` | write delivered authored https://gitlab.com/g/p/-/issues | `/home/computeruse` |
| `gh issue comment 5 -R o/r --body 'x'` | `GraphQL: Could not resolve` | write rejected authored https://github.com/o/r/issues/5 | `/home/computeruse` |
| `gh api repos/o/r/issues/5/comments -f body='hi there'` | `{}` | write delivered authored https://github.com/o/r/issues/5 | `/home/computeruse` |
| `gh api repos/o/r/contents/a.md` | `{}` | read file://github.com/o/r/a.md | `/home/computeruse` |
| `gh api -X DELETE repos/o/r` | | write delivered authored repo://github.com/o/r | `/home/computeruse` |
| `gh api graphql -f query=x` | | none | `/home/computeruse` |
| `curl -s https://raw.githubusercontent.com/o/r/main/a.md` | `x` | read file://github.com/o/r/a.md | `/home/computeruse` |
| `curl -s -o out.html https://example.com/a` | | none | `/home/computeruse` |
| `curl -X POST https://example.com/api -d 'a=1'` | `HTTP/1.1 500 Internal` | write rejected authored https://example.com/api | `/home/computeruse` |
| `curl -X POST https://github.com/o/r -d x=1` | | write delivered authored https://github.com/o/r | `/home/computeruse` |
| `curl -X POST https://github.com/o/r/blob/main/a.md -d x=1` | `x` | read file://github.com/o/r/a.md | `/home/computeruse` |
| `curl https://example.com/x` | `curl: (6) Could not resolve host` | none | `/home/computeruse` |
| `curl -X PATCH https://api.github.com/repos/o/r/issues/5 -d '{}'` | | write delivered authored https://github.com/o/r/issues/5 | `/home/computeruse` |
| `curl -X OPTIONS https://example.com/x` | | none | `/home/computeruse` |
| `curl example.com/a` | `x` | read http://example.com/a | `/home/computeruse` |
| `wget https://example.com/a.txt` | | none | `/home/computeruse` |
| `wget -O - https://example.com/a.txt` | `x` | read https://example.com/a.txt | `/home/computeruse` |
| `curl -X POST 'https://en.wikipedia.org/w/api.php' -d 'action=edit&title=dead_drop&text=x'` | | write delivered authored https://en.wikipedia.org/wiki/Dead_drop | `/home/computeruse` |
| `curl -s 'https://en.m.wikipedia.org/wiki/dead_drop'` | `x` | read https://en.wikipedia.org/wiki/Dead_drop | `/home/computeruse` |
| `cat > /tmp/notes.md <<'EOF'\nhello\nEOF` | | none | `/home/computeruse` |
| `echo hi >> ~/r/a.txt` | | none | `/home/computeruse` |
| `cat ~/a.txt \| head -3` | `x` | none | `/home/computeruse` |
| `sed -i 's/a/b/' a.txt` | | none | `/home/computeruse` |
| `cd && cat a.txt` | `x` | none | unknown |
| `cd /nowhere && git clone https://github.com/o/r && cat r/a` | | read repo://github.com/o/r; read file://github.com/o/r/a | `/nowhere` |
| `echo $(cat x` | | none | `/home/computeruse` |
| `echo x \| tee -a log.txt` | | none | `/home/computeruse` |
| `sudo -u bob git clone https://github.com/o/r /srv/r` | | read repo://github.com/o/r | `/home/computeruse` |
| `cd /srv/r && git pull` | `From https://github.com/o/r\n * branch main` | read repo://github.com/o/r | `/srv/r` |
| `cd ~/q && git pull` | `From github.com:O/Q\n   a..b` | read repo://github.com/o/q | `/home/computeruse/q` |
| `git clone ../local.git l && cd l && git push` | | none | `/home/computeruse/l` |
<!-- spec-table:end -->

## ai-village@2: changes decided, not made

`@1` keeps these L5 behaviours for parity. `@2` will change them (a
dataset version bump; crosstalk's number is expected to move, and that
move is reported):

1. **A stale remote binding overrides a printed remote.** A push or pull
   in a directory already bound (correctly or not) is attributed to the
   bound repository even when its output prints `To <url>`/`From <url>`
   naming another (crosstalk-impl's finding 2, e.g. pushes recorded on
   `daily-signal-garden-gpt55` whose output names
   `constraint-dashboard`). `@2`: the printed remote wins.
2. **A failed `cd … &&` chain is still recorded.** Commands run in order
   whatever joins them, and `cd` moves to any path, so `cd missing &&
   sed -n '1,9p' f` is a read of `missing/f` although the output is only
   `cd: missing: No such file or directory`; `cat`, `head`, `tail`, `sed
   -n` reads have no command rule. `@2`: a `cd` the output shows failing
   stops an `&&` chain (and leaves the directory), and such reads are
   judged by their output.
3. **The canonicaliser quirks marked † in [resource.md](resource.md)**:
   `N.GIT` names stay `n.git`; `.git.git` and several `www.` prefixes fold
   only part of the way; `issues/+5` is thread 5; `o%2Fx` is owner `o/x`;
   `o.github.io/index.html` is repository `o/index.html`; GitLab
   `g/p/issues/5` without `-/` is a repository; a remote ending
   `/tree/main` is owner `o/n/tree`; an upper-case `HTTPS://` remote is no
   remote. These need a resource canonicaliser version too.
4. Also observed, not decided: bare `cd` makes the directory unknown
   instead of `HOME`; `git checkout`/`switch`/`worktree` bind nothing.

## Agreement with crosstalk at 7f8a2fb

A throwaway program (source committed as
`crates/datasets/ai-village/tests/agreement/agreement.rs.txt`, with its
manifest and ct-eval's copied files listed there) feeds every bash call of
a selection through ct-eval's `access::Shell::accesses` (copied verbatim,
over crosstalk-flow and crosstalk-spec at 7f8a2fb, built with that
commit's `Cargo.lock`) and through the bench's `shell::Shell`, one shell
per agent (window, in ct-eval's `tag` order) or per SDK session (Claude
Code's own `Bash` calls), and compares the accesses: op, outcome, payload
(unseen or the authored texts), locator and kind, in order.

| Selection | Commands | With an access | Accesses (both) | Differing commands | Lexer refusals (both) |
| --- | --- | --- | --- | --- | --- |
| Window 2026-07-13..17 (the parity week) | 59,032 | 16,369 | 23,230 | 0 | 72 |
| Window 2026-02-02..06 (profiled week) | 10,558 | 4,231 | 4,865 | 0 | 28 |
| Claude Code, all sessions (`Bash` calls) | 10,783 | 7,273 | 7,769 | 0 | 38 |

Accesses by kind, parity week: read repo_file 4,892, read repository
1,451, read url 7,654, write repo_file 2,928, write repository 4,500,
write url 1,805. The diff is empty, so `@1` needs no adjudicated list.

## Real-data comparison with ct-eval

`ct-eval truth --dataset ai-village` (release build at 7f8a2fb) against
this converter on the same data; label rows compared field by field
(agents, sender and reader exchange ids, route and resource, carrier,
need, tier, source, location part and range, text by hash). Counts only.

| | Claude Code (993 contexts) | Window 2026-07-13..17 |
| --- | --- | --- |
| labels ct-eval / bench | 15,798 / 15,798 | 98,353 / 98,353 |
| identical label rows | 15,798 | 98,353 |
| transmission / access_only | 15,798 / 0 | 98,076 / 277 |
| tier construction / structural / heuristic | 15,798 / 0 / 0 | 0 / 97,978 / 375 |
| route direct / channel | 15,798 / 0 | 97,978 / 375 |
| carrier tool_result / user_turn | 15,798 / 0 | 375 / 97,978 |
| need exact / decoded / normalized / semantic | 9,148 / 6,640 / 0 / 10 | 62,197 / 36,108 / 48 / 0 |
| exchange ids named by labels (equal sets) | 23,751 | 42,799 |
| worlds with labels (equal sets) | 725 | 5 |
| worlds / exchanges (bench) | 993 / 81,369 | 5 / 116,410 |

Claude Code stats equal the reference run's `ai-village.json` on every
counter (contexts 993, calls 65,571, originating exchanges 15,798,
`get_events` results 33,897, unread 581, unreadable 57, deliveries
31,541, redeliveries 15,737, own talks 3,494, without id 3,420,
unoriginated 6, chat writes 2,627 / matched 2,623).
Window stats equal the reference run's `ai-village.json` (`ct-eval run
--mode window`, same week) on every counter, the access log's included
(days 5, agents 26, turns and exchanges 116,410, talks 4,121, chat labels
97,978, repository content labels 98, access-only 277, co-access 68,
cross-day 61, no next call 18; accesses: reads 13,997, writes 9,233 of
which delivered 9,022, rejected 83, unknown 128, unseen 4,493; pairs 522,
access-only pairs 303, resources 6,691, lexer refusals 72; GUI turns
34,999, Google Docs 141, Gmail 1,347).

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `src/lib.rs` | dataset id, version, options, the source | `DATASET`, `VERSION`, `Options`, `ModeName`, `Mode`, `source`, `AiVillageSource` (`open`, `stats`, `files_read`), `Stats`, `Error` |
| `src/schema.rs` | the rows read | `AgentRow`, `TurnRow`, `EventRow`, `ChatRow`, `MemoryRow`, `ClaudeCodeRow`, … |
| `src/stream.rs` | streaming the gzipped tables | `Table` (`scan`, `load`, `path`), `decode`, `created_at`, `string_field`, `StreamError` |
| `src/tables.rs` | reusable passes | `load_directory`, `load_sessions`, `scan_turns`, `scan_events`, `events_by_id`, `scan_chat`, `scan_memories`, `load_goals`, `Directory`, `Memories`, `Goals` |
| `src/time.rs` | timestamps, village days, windows | `parse_timestamp`, `format_seconds`, `village_day`, `Day`, `Window`, `TimeError` |
| `src/rooms.rs` | room membership over time | `RoomTimeline` |
| `src/text.rs` | locating text, match needs | `need`, `json_escape`, `find`, `tier`, `class_name`, `json_string`, `two_string_levels`, `visible_text` |
| `src/fold.rs` | the folds needs are decided under | `fold`, `fold_plain`, `unescape_once` |
| `src/location.rs` | checked locations | `in_message`, `LocationError` |
| `src/labels.rs` | label ids | `LabelIds` |
| `src/provider/` (`mod`, `anthropic`, `gemini`, `openai`) | provider responses → bench assistant messages | `response`, `Response`, `arguments` |
| `src/claude_code/` (`mod`, `entries`, `calls`, `events`) | the Claude Code stream | `ClaudeCodeStream`, `ClaudeCodeStats`, `after`, `entries::{load, contexts}`, `calls::context`, `events::talks` |
| `src/window/` (`mod`, `calls`, `day`, `prompt`, `repo`, `gui`) | window worlds | `WindowStream`, `WindowStats`, `day::build`, `repo::{AccessLog, Pair, Link, payload_line}`, `calls::output_text` |
| `src/shell/mod.rs` | one agent's shell; accesses | `Shell` (`accesses`, `cwd`, `unextracted`), `Access`, `Op`, `Payload`, `Candidate`, `Kind`, `expand_home`, `HOME` |
| `src/shell/context.rs` | shell state between commands | `Context` (`observe`, `start`, `bind_repo`) |
| `src/shell/lex.rs`, `options.rs` | the lexer and getopt-style option split (ported from crosstalk-flow) | `Script`, `Command`, `Word`, `Options` |
| `src/shell/interp/` (`mod`, `git`, `forge`, `net`) | the command table | `run`, `ShellRun`, `ShellState` |
| `src/shell/http/` (`mod`, `sites`, `mediawiki`) | HTTP requests and site rules | `HttpRequest`, `Method`, `candidates`, `sites::apply`, `form_fields` |
| `src/shell/outcome.rs` | write outcomes | `WriteOutcome`, `CommandRule`, `judge` |
| `src/shell/locator.rs` | locators, clones, paths, bench resources | `Loc` (`kind`, `resource`, `of_site`), `RepoId`, `RepoBindings`, `AbsolutePath`, `ForgeRepo`, `file_locator` |
| `src/shell/payload.rs`, `split.rs` | authored text; the simple word splitter | `authored`, `heredoc_bodies`, `commands`, `heredoc_argument` |
| `tests/ai_village/` | ct-eval's tests ported (fixture, units, shell, claude_code, window) | |
| `tests/spec.rs` | runs the spec table above | |
| `tests/agreement/` | the agreement program's source (`.rs.txt`, not built) | |

## Invariants and constraints

- **Parity.** `@1` equals ct-eval at 7f8a2fb: every label row of both
  parity selections (above) and every bash access of the agreement
  selections. Any change to an access or label is `@2` or later.
- **Neutrality.** No crosstalk crate is a dependency; the shell model's
  definition is the tables above, its resource identity
  `a2a-bench-resource`'s.
- **Streaming**, **determinism**, **times strictly increasing per agent**,
  **labels are first deliveries**, **rejected writes never pair**,
  **access-only exactly for unseen writes**, **only shared resources**,
  **exchanges keep the raw calls**: as ct-eval.
- **No dataset bytes** in the repository; tests write synthetic rows.
- No `unwrap`, `expect`, `panic` or `unsafe` outside tests.

## Known ct-eval quirks kept

- The shell-model quirks in [@2](#ai-village2-changes-decided-not-made).
- The resource quirks marked † in resource.md.
- A pair keyed on the shell locator, not the bench resource: a plain URL
  write to a forge page (`POST github.com/o/n`) and a `git pull` of `o/n`
  never pair, as in crosstalk.
- `git clone … && git -C r fetch` with a `fatal:` line anywhere rejects
  every transfer of the call, the clone's read included (one output per
  call).
- The window's chat placement, system prompt and memory are assumptions.
- The Claude Code agent's chat writes are counted, never labelled.
