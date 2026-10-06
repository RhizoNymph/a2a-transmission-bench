//! MediaWiki: every way to read or edit a page, as the page's canonical
//! article URL, with the method and form fields a request carries.
//!
//! The sites and the `GET` rules are `a2a-bench-resource`'s (the resource
//! table's MediaWiki rule); this module adds what a `GET` cannot show: an
//! edit through `api.php` (`action=edit|delete|protect|undelete`, `move`)
//! or `index.php` (`action=submit`, or `edit` with a writing method)
//! writes the page, with its title in the query or in the form body.
//!
//! | Request | Access | Pages |
//! | --- | --- | --- |
//! | `<article path><title>` | write for `action=submit` (or `edit` with a writing method), else read | the title |
//! | `/api/rest_v1/page/<endpoint>/<title>` | read | the title |
//! | `<script>rest.php/v1/page/<title>` | write with a writing method, else read | the title |
//! | `<script>index.php` | as the article path | `title` |
//! | `<script>api.php` `action=edit\|delete\|protect\|undelete` | write | `title` |
//! | `<script>api.php` `action=move` | write | `from` and `to` |
//! | `<script>api.php` `action=query` | read | each of `titles` split on `\|`, or the URL itself |
//! | `<script>api.php` `action=parse` | read | `page`, or the URL itself |
//! | `<script>api.php` another action | no site rule with a writing method, else read of the URL itself | |

use super::super::Kind;
use super::super::locator::Loc;
use super::HttpRequest;
use super::sites::{SiteAccess, percent_decode};

/// One MediaWiki site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    /// Host patterns: a host, or `*.suffix` for the suffix and every
    /// subdomain of it.
    pub hosts: &'static [&'static str],
    /// `$wgArticlePath` without `$1`.
    pub article_path: &'static str,
    /// `$wgScriptPath` with a trailing slash.
    pub script_path: &'static str,
    /// `$wgCapitalLinks`.
    pub capital_links: bool,
    /// `x.m.y` is the same site as `x.y`.
    pub fold_mobile_host: bool,
}

/// The sites, in the order their rules are tried.
pub const SITES: [Site; 3] = [
    Site {
        hosts: &[
            "*.wikipedia.org",
            "*.wikimedia.org",
            "*.wikibooks.org",
            "*.wikiquote.org",
            "*.wikisource.org",
            "*.wikiversity.org",
            "*.wikivoyage.org",
            "*.wikinews.org",
            "*.wikidata.org",
            "*.mediawiki.org",
        ],
        article_path: "/wiki/",
        script_path: "/w/",
        capital_links: true,
        fold_mobile_host: true,
    },
    Site {
        hosts: &["*.wiktionary.org"],
        article_path: "/wiki/",
        script_path: "/w/",
        capital_links: false,
        fold_mobile_host: true,
    },
    Site {
        hosts: &["*.fandom.com"],
        article_path: "/wiki/",
        script_path: "/",
        capital_links: true,
        fold_mobile_host: false,
    },
];

fn matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => {
            host == suffix
                || host
                    .strip_suffix(suffix)
                    .is_some_and(|rest| rest.ends_with('.'))
        }
        None => host == pattern,
    }
}

/// The sites `host` (without its port) belongs to; `None` when it is no
/// wiki's.
pub fn sites_of(host: &str) -> Option<Vec<Site>> {
    let sites: Vec<Site> = SITES
        .iter()
        .filter(|site| site.hosts.iter().any(|pattern| matches(pattern, host)))
        .copied()
        .collect();
    (!sites.is_empty()).then_some(sites)
}

const REST_PAGE: &str = "rest.php/v1/page/";
const WIKIMEDIA_REST_PAGE: &str = "/api/rest_v1/page/";

/// The pages `request` reads or edits on `site`, `None` when it is not a
/// page request this rule knows.
pub fn apply(site: &Site, request: &HttpRequest) -> Option<SiteAccess> {
    let Loc::Url {
        scheme, host, path, ..
    } = &request.url
    else {
        return None;
    };
    let host = if site.fold_mobile_host {
        fold_mobile(host)
    } else {
        host.clone()
    };
    let page = |title: &str| page_locator(site, scheme, &host, title);
    let params = request.params();
    let param = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let page_kind = || match param("action") {
        Some("submit") => Kind::Write,
        Some("edit") if request.method.writes() => Kind::Write,
        _ => Kind::Read,
    };
    let access = |kind, locators: Vec<Loc>| Some(SiteAccess { kind, locators });

    if let Some(title) = path.strip_prefix(site.article_path) {
        return access(page_kind(), vec![page(&percent_decode(title))?]);
    }
    if let Some(rest) = path.strip_prefix(WIKIMEDIA_REST_PAGE) {
        let (_endpoint, rest) = rest.split_once('/')?;
        let title = rest.split('/').next()?;
        return access(Kind::Read, vec![page(&percent_decode(title))?]);
    }
    let script = path.strip_prefix(site.script_path)?;
    if let Some(rest) = script.strip_prefix(REST_PAGE) {
        let title = rest.split('/').next()?;
        let kind = if request.method.writes() {
            Kind::Write
        } else {
            Kind::Read
        };
        return access(kind, vec![page(&percent_decode(title))?]);
    }
    match script {
        "index.php" => access(page_kind(), vec![page(param("title")?)?]),
        "api.php" => match param("action")? {
            "edit" | "delete" | "protect" | "undelete" => {
                access(Kind::Write, vec![page(param("title")?)?])
            }
            "move" => access(
                Kind::Write,
                vec![page(param("from")?)?, page(param("to")?)?],
            ),
            "query" => match param("titles") {
                Some(titles) => access(Kind::Read, titles.split('|').filter_map(page).collect()),
                None => access(Kind::Read, vec![request.url.clone()]),
            },
            "parse" => match param("page") {
                Some(title) => access(Kind::Read, vec![page(title)?]),
                None => access(Kind::Read, vec![request.url.clone()]),
            },
            _ if request.method.writes() => None,
            _ => access(Kind::Read, vec![request.url.clone()]),
        },
        _ => None,
    }
}

/// `raw` as MediaWiki canonicalizes a title, `None` when nothing is left.
pub fn canonical_title(raw: &str, capital_links: bool) -> Option<String> {
    let raw = raw.split('#').next().unwrap_or_default();
    let words: Vec<&str> = raw
        .split(|c: char| c == '_' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return None;
    }
    let title = words.join(" ");
    if !capital_links {
        return Some(title);
    }
    let mut chars = title.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// The canonical article URL of `title` on `site`.
pub fn page_locator(site: &Site, scheme: &str, host: &str, title: &str) -> Option<Loc> {
    let title = canonical_title(title, site.capital_links)?;
    let mut path = String::from(site.article_path);
    for c in title.chars() {
        match c {
            ' ' => path.push('_'),
            '%' => path.push_str("%25"),
            '?' => path.push_str("%3F"),
            '#' => path.push_str("%23"),
            '\\' => path.push_str("%5C"),
            c => path.push(c),
        }
    }
    Loc::normalized(&format!("{scheme}://{host}{path}"))
}

/// `en.m.wikipedia.org` as `en.wikipedia.org`: an `m` label after the
/// first is the mobile site.
fn fold_mobile(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').collect();
    match labels.as_slice() {
        [first, "m", rest @ ..] if !rest.is_empty() => std::iter::once(*first)
            .chain(rest.iter().copied())
            .collect::<Vec<_>>()
            .join("."),
        ["m", rest @ ..] if rest.len() > 1 => rest.join("."),
        _ => host.to_owned(),
    }
}
