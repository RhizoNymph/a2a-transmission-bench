# Resource canonicaliser (`a2a-bench-resource`)

The bench's own definition of when two resources are the same: a channel
label and a prediction align only if they name the same canonical
resource (design §6.1). The crate `a2a-bench-resource`
(`crates/resource`) is its only implementation. Converters make labels'
resources with it, and the scorer applies `canonicalize` to labels and
predictions alike before comparing them, so no detector's extractor
decides what a label's resource is.

**Version 1 equals crosstalk at commit `7f8a2fb`** (crosstalk-flow's
`tool_url_locator`, `url_locator`, `SitesConfig::default()`, `RepoId`
and crosstalk-spec's `Locator::repository`, as the AI Village converter
`datasets/ai_village/resource.rs` used them), so the AI Village labels come
out unchanged. The [spec table](#spec-table) below is normative; parity
with crosstalk is checked by vectors crosstalk computed (see
[Parity](#parity)). There are no intentional differences from crosstalk's
functions.

## Scope

- `canonical_url(text)`: the resource a `GET` of a URL reaches. URL
  normalization, scheme-less hosts, invalid hosts, and the site rules:
  GitHub and GitLab web, raw, API, codeload and Pages forms; MediaWiki
  article, index, API and REST forms.
- `normalized_url(text)`: URL normalization alone (no site rule, no assumed
  scheme), for converters whose truth names a URL as written (demo-swarm).
- `canonical_remote(text)`: the forge repository a git remote names
  (https, `ssh://`, `git://`, scp-like `git@host:o/n.git`).
- `repository(host, owner, name)`: a repository's canonical parts.
- `canonicalize(&Resource)`: idempotent canonical form of any resource.
- `kind(&Resource)`: whether a resource is shared, and as what.

## Non-scope

- Shell commands, working directories, clone bindings and which file a
  path names inside a clone: the AI Village converter's (`datasets::
  ai_village::shell`, design §5.2).
- Tool-call accesses (which argument names the URL, read or write): the
  converters' and detectors'.
- Writing requests to a site: every rule here reads the URL as a `GET`.
- Threads on hosts other than `github.com` and `gitlab.com` (GitHub
  Enterprise, self-hosted GitLab): their URLs stay URLs, as in crosstalk.
- The `Resource` shape itself: `a2a-bench-format` (`src/resource.rs`).

## Data and control flow

```text
canonical_url(text)
  └─ url::normalize_tool(text)                text trimmed
       ├─ url::normalize(text)                Url::parse (WHATWG) ─▶ NormalUrl {scheme, host[:port], path, query}
       │     └─ parse error on a host ─▶ invalid_host::key ─▶ Opaque {"<url>", key}
       └─ on error: bare::host(text) ─▶ normalize("https://" + text)
  └─ NormalUrl ─▶ sites::apply                 MediaWiki sites (by host pattern), then GitHub, then GitLab
       ├─ a rule's resource (Repository, RepoFile, Thread, Collection, wiki article Url)
       └─ none ─▶ Url(NormalUrl::text())

canonical_remote(text) ─▶ repository::remote ─▶ repository(host, owner, name)

canonicalize(resource) = step(step(…)) until unchanged (at most 16 steps)
  step: Repository/RepoFile/Thread/Collection ─▶ repository(..) on the parts; RepoFile path rooted, resolved
        Thread pull ─▶ issue; Url(text) ─▶ canonical_url(text) (unchanged when it is no URL)
        File absolute path ─▶ resolved; Opaque unchanged
```

## Rules

### URLs (`normalized_url`, and the first stage of `canonical_url`)

1. Surrounding whitespace is trimmed; the text is parsed as a WHATWG URL
   (`url` 2.5.8): scheme lower case; a special scheme's host lower case
   and IDNA-mapped to punycode (`bücher.example` → `xn--bcher-kva.example`),
   IPv4 forms made dotted-decimal; default port dropped; `.`/`..` path
   segments resolved; characters outside the URL code points
   percent-encoded.
2. A URL with no host, or an empty one (`mailto:`, `file:///x`), is no
   resource (`UrlError::NoHost`).
3. User info and the fragment are dropped.
4. The host is lower-cased (also for non-special schemes); one trailing
   dot is dropped; a non-default port is kept as `host:port`.
5. Percent-encoding in path and query is normalized: an escaped unreserved
   character (`A-Z a-z 0-9 - . _ ~`) is decoded, every other escape gets
   upper-case hex, a `%` without two hex digits stays.
6. The query is split on `&`, empty parameters dropped, each parameter
   kept as written (after rule 5) and the list sorted bytewise; an empty
   query is no query.
7. An empty path is `/`. The canonical text is
   `{scheme}://{host}{path}[?{query}]`. `www.` is **not** dropped from a
   URL's host (only from a repository's).
8. A `scheme://` URL whose host fails to parse (IDNA, a forbidden domain
   character, a bad IPv4 or IPv6 address) is `Opaque { tool: "<url>",
   key }`, the key being the text trimmed, scheme ASCII-lower-cased and
   host part Unicode-lower-cased, user info and fragment dropped, the rest
   as written.
9. (`canonical_url` only) When parsing fails or gives no host, text with
   no `://` and no whitespace whose authority is a domain (labels of
   letters, digits and inner hyphens, at most 63 characters each and 253 in
   all, a final alphabetic label of two or more characters, an optional
   trailing dot) and an optional port of one to five digits is read as
   `https://` + the text. A bare name whose last label is a common file
   extension (`README.md`, `main.rs`; the list is in `src/url/bare.rs`)
   with no `www.`, port or path is not a host.

### Repositories (`repository`)

1. Host: up to the first `:`, trailing dots dropped, ASCII lower case, one
   leading `www.` dropped; must be non-empty ASCII letters, digits, `.`
   and `-`, not starting with `.`.
2. Owner: slashes around it trimmed, split on `/`, each segment ASCII
   lower case and non-empty, not `.` or `..`, of ASCII letters, digits,
   `-`, `_` and `.`; nested groups are joined with `/`.
3. Name: slashes around it trimmed, one `.git` suffix dropped, ASCII lower
   case, one segment as above.

### Site rules (`canonical_url`, after the URL rules)

MediaWiki sites are tried first, then GitHub, then GitLab; only the first
resource a rule names counts. The URL's path is split into its non-empty
segments, each percent-decoded (text that does not decode to UTF-8 stays
encoded).

- **GitHub** (`github.com`, `www.github.com`; `codeload.github.com`;
  `raw.githubusercontent.com`; `api.github.com`; `<o>.github.io`). A
  first segment in `about apps collections contact customer-stories
  enterprise events explore features issues login marketplace new
  notifications orgs pricing pulls search settings sponsors topics users`
  is no repository. Web `o/n` and `o/n/tree/<ref>` (exactly) are the
  repository; `o/n/blob|raw/<ref>/<path…>` the file; `o/n/issues|pull/<N>/…`
  the thread `issue N` (issues and pull requests share numbers and one
  conversation); `o/n/issues|pulls` the collection. codeload `o/n/…` is the
  repository. raw `o/n/refs/heads|tags/<ref>/<path…>` or `o/n/<ref>/<path…>`
  the file. API `repos/o/n/contents/<path…>` the file,
  `repos/o/n/issues|pulls/<digits>/…` the thread, `repos/o/n/issues|pulls`
  the collection, any other `repos/o/n/…` the repository. Pages
  `<o>.github.io/<n>/…` is `o/n`, bare `<o>.github.io/` is
  `o/<o>.github.io`. Every repository is on host `github.com`.
- **GitLab** (`gitlab.com`, `www.gitlab.com`; `<g>.gitlab.io`). API
  `api/v4/projects/<encoded path>/…`: `repository/files/<encoded
  file>(/raw)` the file, `issues|merge_requests/<digits>/…` the thread,
  `issues|merge_requests` the collection, anything else the repository; a
  numeric project id is no repository. A first segment in `- api dashboard
  explore groups help users search` is no repository. Web: the segments
  before a `-` segment (all of them when there is none) are the project
  path (owner = all but the last); after `-`, `blob|raw/<ref>/<file…>` the
  file, `issues|merge_requests/<N>/…` the thread, `issues|merge_requests`
  the collection, anything else the repository. Pages `<g>.gitlab.io/<p>/…`
  is `g/p`; a bare `<g>.gitlab.io/` and a unique domain
  `<name>-<6 hex>.gitlab.io` are no repository.
- **MediaWiki** (`*.wikipedia.org *.wikimedia.org *.wikibooks.org
  *.wikiquote.org *.wikisource.org *.wikiversity.org *.wikivoyage.org
  *.wikinews.org *.wikidata.org *.mediawiki.org` with capital links,
  `*.wiktionary.org` without, both with articles at `/wiki/`, scripts at
  `/w/` and the mobile host `x.m.y` folded to `x.y`; `*.fandom.com` with
  capital links, scripts at `/`, no mobile folding). The page named by
  `/wiki/<title>`, `/api/rest_v1/page/<endpoint>/<title>`,
  `<script>rest.php/v1/page/<title>`, `<script>index.php?title=`, or
  `<script>api.php` with `action=edit|delete|protect|undelete` (`title`),
  `move` (`from`), `query` (the first of `titles` split on `|`), `parse`
  (`page`) is the URL of its article: title canonical (`_` and whitespace
  runs one space, trimmed, `#…` dropped, first letter upper-cased on a
  capital-links site), spaces as `_`, `% ? # \` escaped, on the request's
  scheme and (folded) host. `api.php` with another action, or `query` or
  `parse` without its title parameter, is the URL itself.
- The thread number is a `u64` as Rust parses it (`+5` is 5; `05` is 5);
  a thread or file that does not resolve (number out of range, no file
  path, an invalid repository) makes the rule name nothing, and the URL
  stays a URL.
- File paths are rooted at `/` and resolved lexically (`.`, `..`, empty
  segments).

### Remotes (`canonical_remote`)

Trimmed; text with whitespace is no remote. `http https ssh git git+ssh
ssh+git` `://[user@]host[:port]/path`: the repository at `path` on
`host`. `file://…`, an absolute path and `./…` are local (no forge).
Scp form `[user@]host:path` (host without `/`, path not starting with
`//`) whose host has a dot or is `localhost`: the repository at `path`.
The path's slashes around it and one `.git` are dropped; its last segment
is the name, everything before it the owner (so `…/o/n/tree/main` is
owner `o/n/tree`, name `main`). Any other text is no remote.

### Canonical resources (`canonicalize`, `kind`)

`canonicalize` repeats one step until nothing changes (one step is enough
except for a repository that `repository` folds only part of the way: a
host with several `www.` prefixes, a name ending in an upper-case `.GIT`,
which is lower-cased after the suffix check, or in `.git.git`):
repositories through `repository` (unchanged when invalid); a repository
file's path rooted at `/` and resolved; thread kind `pull` is `issue`;
`Url(text)` is `canonical_url(text)` (unchanged when it is no URL); an
absolute `File` path resolved; `Opaque` unchanged. Collections keep their
kind: `pulls` (GitHub) and `merge_requests` (GitLab) are different pages.
`kind`: `repository` → repository; `repo_file` → repo file; `thread`,
`collection`, `url` → url; `file`, `opaque` → none (a file only one
agent's machine holds; a tool's key).

## Spec table

Every row is a test (`tests/spec.rs` reads this table), and every input of
`canonical_url`, `normalized_url` and `canonical_remote` is also one of the
parity vectors. Outputs are written `repository <host>/<owner>/<name>`,
`repo_file <repo> <path>`, `thread <repo> <issue|pull|merge_request> <N>`,
`collection <repo> <issues|pulls|merge_requests>`, `url <text>`,
`file <path>`, `opaque <tool> <key>`, `error <reason>`; `canonicalize` and
`kind` take inputs in the same notation. Rows marked † are crosstalk
behaviour that looks surprising; they are kept in version 1 for parity.

<!-- spec-table:start -->
| Function | Input | Output |
| --- | --- | --- |
| `canonical_url` | `https://github.com/o/n` | `repository github.com/o/n` |
| `canonical_url` | `https://GitHub.com/AgentVillage/Atlas.git` | `repository github.com/agentvillage/atlas` |
| `canonical_url` | `http://www.github.com/o/n/` | `repository github.com/o/n` |
| `canonical_url` | `github.com/o/n` | `repository github.com/o/n` |
| `canonical_url` | `ssh://git@github.com/o/n.git` | `repository github.com/o/n` |
| `canonical_url` | `https://github.com/o/N.GIT` | `repository github.com/o/n.git` † |
| `canonical_url` | `https://github.com/o/n.git.git` | `repository github.com/o/n` † |
| `canonical_url` | `https://github.com/o/n/tree/main` | `repository github.com/o/n` |
| `canonical_url` | `https://github.com/o/n/tree/feature/x` | `url https://github.com/o/n/tree/feature/x` |
| `canonical_url` | `https://github.com/o/n/commits/main` | `url https://github.com/o/n/commits/main` |
| `canonical_url` | `https://github.com/o/n/blob/main/src/lib.rs` | `repo_file github.com/o/n /src/lib.rs` |
| `canonical_url` | `https://github.com/o/n/blob/feature/x/src/lib.rs` | `repo_file github.com/o/n /x/src/lib.rs` |
| `canonical_url` | `https://github.com/o/n/raw/3f2c1a9/a/../b.md` | `repo_file github.com/o/n /b.md` |
| `canonical_url` | `https://github.com/o/n/blob/main` | `url https://github.com/o/n/blob/main` |
| `canonical_url` | `https://raw.githubusercontent.com/O/N/main/README.md` | `repo_file github.com/o/n /README.md` |
| `canonical_url` | `https://raw.githubusercontent.com/o/n/refs/heads/main/docs/a.md` | `repo_file github.com/o/n /docs/a.md` |
| `canonical_url` | `https://api.github.com/repos/o/n/contents/src/app.py?ref=main` | `repo_file github.com/o/n /src/app.py` |
| `canonical_url` | `https://api.github.com/repos/o/n` | `repository github.com/o/n` |
| `canonical_url` | `https://api.github.com/repos/o/n/commits?per_page=5` | `repository github.com/o/n` |
| `canonical_url` | `https://codeload.github.com/o/n/zip/refs/heads/main` | `repository github.com/o/n` |
| `canonical_url` | `https://github.com/o/n/issues/12` | `thread github.com/o/n issue 12` |
| `canonical_url` | `https://github.com/o/n/pull/7/files` | `thread github.com/o/n issue 7` |
| `canonical_url` | `https://api.github.com/repos/o/n/pulls/7/comments` | `thread github.com/o/n issue 7` |
| `canonical_url` | `https://github.com/o/n/issues/012#issuecomment-1` | `thread github.com/o/n issue 12` |
| `canonical_url` | `https://github.com/o/n/issues/+5` | `thread github.com/o/n issue 5` † |
| `canonical_url` | `https://github.com/o/n/issues` | `collection github.com/o/n issues` |
| `canonical_url` | `https://github.com/o/n/pulls` | `collection github.com/o/n pulls` |
| `canonical_url` | `https://api.github.com/repos/o/n/issues?state=open` | `collection github.com/o/n issues` |
| `canonical_url` | `https://github.com/o/n/issues/new` | `url https://github.com/o/n/issues/new` |
| `canonical_url` | `https://github.com/settings/profile` | `url https://github.com/settings/profile` |
| `canonical_url` | `https://github.com/o%2Fx/n` | `repository github.com/o/x/n` † |
| `canonical_url` | `https://github.com:8443/o/n` | `url https://github.com:8443/o/n` |
| `canonical_url` | `https://o.github.io/n/page.html` | `repository github.com/o/n` |
| `canonical_url` | `https://O.github.io/` | `repository github.com/o/o.github.io` |
| `canonical_url` | `https://o.github.io/index.html` | `repository github.com/o/index.html` † |
| `canonical_url` | `https://gitlab.com/g/s/p` | `repository gitlab.com/g/s/p` |
| `canonical_url` | `https://gitlab.com/G/P.git` | `repository gitlab.com/g/p` |
| `canonical_url` | `https://gitlab.com/g/p/-/tree/main` | `repository gitlab.com/g/p` |
| `canonical_url` | `https://gitlab.com/g/p/-/blob/main/src/x.py` | `repo_file gitlab.com/g/p /src/x.py` |
| `canonical_url` | `https://gitlab.com/g/p/-/issues/3` | `thread gitlab.com/g/p issue 3` |
| `canonical_url` | `https://gitlab.com/g/p/-/merge_requests/4/diffs` | `thread gitlab.com/g/p merge_request 4` |
| `canonical_url` | `https://gitlab.com/g/p/-/merge_requests` | `collection gitlab.com/g/p merge_requests` |
| `canonical_url` | `https://gitlab.com/api/v4/projects/g%2Fs%2Fp/issues/5` | `thread gitlab.com/g/s/p issue 5` |
| `canonical_url` | `https://gitlab.com/api/v4/projects/g%2Fp/repository/files/src%2Fx.py/raw` | `repo_file gitlab.com/g/p /src/x.py` |
| `canonical_url` | `https://gitlab.com/api/v4/projects/123/issues` | `url https://gitlab.com/api/v4/projects/123/issues` |
| `canonical_url` | `https://gitlab.com/g/p/issues/5` | `repository gitlab.com/g/p/issues/5` † |
| `canonical_url` | `https://gitlab.com/g` | `url https://gitlab.com/g` |
| `canonical_url` | `https://g.gitlab.io/p/index.html` | `repository gitlab.com/g/p` |
| `canonical_url` | `https://g.gitlab.io/` | `url https://g.gitlab.io/` |
| `canonical_url` | `https://site-1a2b3c.gitlab.io/p` | `url https://site-1a2b3c.gitlab.io/p` |
| `canonical_url` | `https://en.wikipedia.org/w/index.php?title=dead+drop&action=raw` | `url https://en.wikipedia.org/wiki/Dead_drop` |
| `canonical_url` | `https://en.m.wikipedia.org/wiki/dead_drop` | `url https://en.wikipedia.org/wiki/Dead_drop` |
| `canonical_url` | `https://en.wiktionary.org/wiki/dead_drop` | `url https://en.wiktionary.org/wiki/dead_drop` |
| `canonical_url` | `https://en.wikipedia.org/w/api.php?action=parse&page=dead_drop&format=json` | `url https://en.wikipedia.org/wiki/Dead_drop` |
| `canonical_url` | `https://en.wikipedia.org/api/rest_v1/page/html/Dead_drop` | `url https://en.wikipedia.org/wiki/Dead_drop` |
| `canonical_url` | `https://en.wikipedia.org/w/api.php` | `url https://en.wikipedia.org/w/api.php` |
| `canonical_url` | `https://community.fandom.com/wiki/help:Contents` | `url https://community.fandom.com/wiki/Help:Contents` |
| `canonical_url` | `HTTPS://Dead-Drops.Example:443/box/./7?b=2&a=1#x` | `url https://dead-drops.example/box/7?a=1&b=2` |
| `canonical_url` | `https://user:secret@example.com/a` | `url https://example.com/a` |
| `canonical_url` | `http://example.com:80/` | `url http://example.com/` |
| `canonical_url` | `https://example.com:8443/a` | `url https://example.com:8443/a` |
| `canonical_url` | `https://example.com./a/./b/../c` | `url https://example.com/a/c` |
| `canonical_url` | `https://example.com/%7euser/%2fx` | `url https://example.com/~user/%2Fx` |
| `canonical_url` | `https://example.com/a?&&b=1&` | `url https://example.com/a?b=1` |
| `canonical_url` | `https://example.com/a?` | `url https://example.com/a` |
| `canonical_url` | `https://www.example.com` | `url https://www.example.com/` |
| `canonical_url` | `https://bücher.example/` | `url https://xn--bcher-kva.example/` |
| `canonical_url` | `https://[::1]:8080/x` | `url https://[::1]:8080/x` |
| `canonical_url` | `www.example.com` | `url https://www.example.com/` |
| `canonical_url` | `example.com:8080/a` | `url https://example.com:8080/a` |
| `canonical_url` | `README.md` | `error parse` |
| `canonical_url` | `mailto:a@example.com` | `error no_host` |
| `canonical_url` | `http://xn--/path` | `opaque <url> http://xn--/path` |
| `canonical_url` | `HTTP://user:pw@XN--/Path#top` | `opaque <url> http://xn--/Path` |
| `normalized_url` | `https://github.com/o/n` | `url https://github.com/o/n` |
| `normalized_url` | `HTTP://Example.COM:80` | `url http://example.com/` |
| `normalized_url` | `github.com/o/n` | `error parse` |
| `normalized_url` | `http://xn--/path` | `opaque <url> http://xn--/path` |
| `canonical_remote` | `git@github.com:O/N.git` | `repository github.com/o/n` |
| `canonical_remote` | `https://github.com/o/n.git` | `repository github.com/o/n` |
| `canonical_remote` | `https://www.github.com/o/n` | `repository github.com/o/n` |
| `canonical_remote` | `ssh://git@gitlab.example.com:2222/g/s/p.git` | `repository gitlab.example.com/g/s/p` |
| `canonical_remote` | `git+ssh://host.example/o/n/` | `repository host.example/o/n` |
| `canonical_remote` | `localhost:o/n` | `repository localhost/o/n` |
| `canonical_remote` | `https://github.com/o/n/tree/main` | `repository github.com/o/n/tree/main` † |
| `canonical_remote` | `file:///srv/repo.git` | `error local` |
| `canonical_remote` | `/srv/repo` | `error local` |
| `canonical_remote` | `./repo` | `error local` |
| `canonical_remote` | `C:/x/y` | `error not_a_remote` |
| `canonical_remote` | `ftp://host.example/o/n` | `error not_a_remote` |
| `canonical_remote` | `HTTPS://github.com/o/n` | `error not_a_remote` † |
| `canonical_remote` | `https://github.com/n` | `error no_owner` |
| `canonical_remote` | `git@github.com:o/n+x` | `error repository` |
| `canonicalize` | `repository GitHub.com./O/N.git` | `repository github.com/o/n` |
| `canonicalize` | `repository github.com/o/n.git` | `repository github.com/o/n` |
| `canonicalize` | `repository www.www.example.com/o/n` | `repository example.com/o/n` |
| `canonicalize` | `repository github.com/o+x/n` | `repository github.com/o+x/n` |
| `canonicalize` | `repo_file github.com/O/n src/./lib.rs` | `repo_file github.com/o/n /src/lib.rs` |
| `canonicalize` | `thread github.com/o/n pull 7` | `thread github.com/o/n issue 7` |
| `canonicalize` | `thread gitlab.com/g/p merge_request 4` | `thread gitlab.com/g/p merge_request 4` |
| `canonicalize` | `collection GitHub.com/O/N pulls` | `collection github.com/o/n pulls` |
| `canonicalize` | `url https://github.com/o/n/pull/7` | `thread github.com/o/n issue 7` |
| `canonicalize` | `url github.com/o/n` | `repository github.com/o/n` |
| `canonicalize` | `url HTTPS://Example.com/b?z=1&a=2` | `url https://example.com/b?a=2&z=1` |
| `canonicalize` | `url not a url` | `url not a url` |
| `canonicalize` | `file /shared/./notes/../a.md` | `file /shared/a.md` |
| `canonicalize` | `file notes.md` | `file notes.md` |
| `canonicalize` | `opaque memory_write Key One` | `opaque memory_write Key One` |
| `kind` | `repository github.com/o/n` | `repository` |
| `kind` | `repo_file github.com/o/n /a` | `repo_file` |
| `kind` | `thread github.com/o/n issue 1` | `url` |
| `kind` | `collection github.com/o/n issues` | `url` |
| `kind` | `url https://example.com/` | `url` |
| `kind` | `file /home/a/x` | `none` |
| `kind` | `opaque <url> http://xn--/path` | `none` |
<!-- spec-table:end -->

## Parity

`tests/fixtures/crosstalk-7f8a2fb-resource-vectors.json` holds, for every
input of the spec table and several hundred more URL and remote forms,
what crosstalk at `7f8a2fb` gives: `url` (the AI Village converter's
`from_url`: `tool_url_locator`, then the first locator of
`SitesConfig::default()` on a `GET`), `plain` (`url_locator`), `remote`
(`from_remote`), `kind`, and `Locator::repository` on (host, owner, name)
triples. `tests/fixtures/crosstalk-resource-vectors-generator.rs.txt` is
the program that computed them (built against crosstalk-flow and
crosstalk-spec at `7f8a2fb`, with that commit's `Cargo.lock`), and
`crosstalk-resource-vectors-inputs.py.txt` builds its inputs from this
table plus 800-odd further GitHub, GitLab, Pages, MediaWiki, generic URL
and remote forms (odd ports, cases, encodings, IDNA and IP hosts,
scheme-less text). The parity
test (`tests/parity.rs`) maps the bench's output to crosstalk's locator
shape (a thread or collection to its canonical page URL, a repository file
to `File { host: "<host>/<owner>/<name>" }`, an invalid-host URL to
`Opaque { tool: "<url>" }`) and requires equality for every vector,
errors included.

## Files

| File | Role | Key exports |
| --- | --- | --- |
| `src/lib.rs` | crate root | the public API below |
| `src/canonical.rs` | entry points over text and resources | `canonical_url`, `normalized_url`, `canonicalize` |
| `src/kind.rs` | shared or not | `ResourceKind`, `kind` |
| `src/error.rs` | typed errors | `UrlError`, `InvalidRepository`, `RemoteError` |
| `src/url/mod.rs` | URL normalization (rules 1–7), canonical text | `NormalUrl`, `Normalized`, `normalize`, `normalize_tool` (crate) |
| `src/url/percent.rs` | percent-encoding normal form, decoding, path segments | crate-internal |
| `src/url/invalid_host.rs` | invalid-host URLs as opaque keys (rule 8) | `INVALID_HOST_TOOL` |
| `src/url/bare.rs` | scheme-less hosts (rule 9) | crate-internal |
| `src/repository/mod.rs` | repository parts in canonical form | `repository` |
| `src/repository/remote.rs` | git remotes | `canonical_remote` |
| `src/repository/path.rs` | lexical absolute paths | crate-internal |
| `src/sites/mod.rs` | the site table (`SitesConfig::default()` as data), host patterns, rule order | crate-internal |
| `src/sites/github.rs`, `gitlab.rs`, `mediawiki.rs` | the site rules | crate-internal |
| `tests/spec.rs` | runs the spec table above | |
| `tests/parity.rs` | equality with crosstalk on every vector; idempotence | |
| `tests/fixtures/crosstalk-7f8a2fb-resource-vectors.json` | vectors crosstalk computed | |
| `tests/fixtures/crosstalk-resource-vectors-generator.rs.txt` | the generator | |
| `tests/fixtures/crosstalk-resource-vectors-inputs.py.txt` | builds the generator's inputs: the spec table's text inputs plus the extra forms | |
| `tests/common/mod.rs` | the spec notation; crosstalk locators in it | `render`, `parse`, `render_as_locator`, `render_locator` |

## Invariants and constraints

- **Parity.** `canonical_url`, `normalized_url`, `canonical_remote` and
  `repository` equal crosstalk's functions at `7f8a2fb` on every vector,
  errors included; `url` is pinned to the same version (2.5.8, with the
  same `idna` and ICU data) since IDNA and WHATWG parsing are part of the
  rules. Any change to an output is a new canonicaliser version and a
  dataset version bump for every dataset whose labels hold resources.
- **Idempotence.** `canonicalize(canonicalize(r)) == canonicalize(r)`, and
  `canonicalize` of any output of `canonical_url` or `canonical_remote` is
  that output, except a repository that crosstalk's single pass folds only
  part of the way (a host with several `www.` prefixes, an upper-case
  `.GIT` name suffix, `.git.git`), which `canonicalize` folds the rest of
  the way; `canonicalize(Url(text))` then equals `canonicalize` of
  `canonical_url(text)` (tested on every vector). Since the scorer
  canonicalizes both sides, these cases still align.
- **Totality.** `canonicalize` never fails: a part that is no repository,
  a path that does not resolve or text that is no URL is kept as it is,
  and can equal only itself.
- **Neutrality.** No dependency on any crosstalk crate; the table above,
  not crosstalk's code, is the definition.
- No `unwrap`, `expect`, `panic` or `unsafe`.
