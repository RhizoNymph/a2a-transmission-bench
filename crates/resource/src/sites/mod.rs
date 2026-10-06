//! Site rules: the pages of known wikis and forges get one resource,
//! whatever URL reaches them.
//!
//! Which hosts are MediaWiki sites, and that GitHub's and GitLab's rules
//! are on, is the table below ([`MEDIAWIKI`]); it is crosstalk-flow's
//! `SitesConfig::default()` as data. MediaWiki sites are tried first, then
//! GitHub ([`github`]), then GitLab ([`gitlab`]). Every rule reads the URL
//! as a `GET`.

mod github;
mod gitlab;
mod mediawiki;

use a2a_bench_format::resource::Resource;

use crate::url::NormalUrl;

/// A MediaWiki site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MediaWikiSite {
    /// Hosts, or `*.suffix` for the suffix and every subdomain of it.
    pub hosts: &'static [&'static str],
    /// `$wgArticlePath` without `$1`.
    pub article_path: &'static str,
    /// `$wgScriptPath` with a trailing slash.
    pub script_path: &'static str,
    /// `$wgCapitalLinks`: the first letter of a title is upper-cased.
    pub capital_links: bool,
    /// The mobile host (`en.m.wikipedia.org`) is the desktop one's site.
    pub fold_mobile_host: bool,
}

/// The MediaWiki sites, in the order they are tried.
pub(crate) const MEDIAWIKI: [MediaWikiSite; 3] = [
    MediaWikiSite {
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
    MediaWikiSite {
        hosts: &["*.wiktionary.org"],
        article_path: "/wiki/",
        script_path: "/w/",
        capital_links: false,
        fold_mobile_host: true,
    },
    MediaWikiSite {
        hosts: &["*.fandom.com"],
        article_path: "/wiki/",
        script_path: "/",
        capital_links: true,
        fold_mobile_host: false,
    },
];

/// The resource the first site rule that recognizes `url` gives it, `None`
/// when no rule does or the rule names no resource.
pub(crate) fn apply(url: &NormalUrl) -> Option<Resource> {
    let name = host_name(&url.host);
    let wiki = MEDIAWIKI
        .iter()
        .filter(|site| site.hosts.iter().any(|pattern| host_matches(pattern, name)))
        .find_map(|site| mediawiki::apply(site, url));
    match wiki {
        Some(pages) => pages.into_iter().next(),
        None => github::apply(url).or_else(|| gitlab::apply(url)),
    }
}

/// A host without its port.
fn host_name(host: &str) -> &str {
    if host.starts_with('[') {
        return host;
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

/// Whether `host` matches `pattern`: equal, or for `*.suffix` the suffix
/// itself or a subdomain of it.
fn host_matches(pattern: &str, host: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{host_matches, host_name};

    #[test]
    fn host_patterns_match_the_suffix_and_subdomains() {
        assert!(host_matches("*.wikipedia.org", "wikipedia.org"));
        assert!(host_matches("*.wikipedia.org", "en.m.wikipedia.org"));
        assert!(!host_matches("*.wikipedia.org", "notwikipedia.org"));
        assert!(host_matches("a.org", "a.org"));
        assert!(!host_matches("a.org", "b.a.org"));
    }

    #[test]
    fn host_name_drops_a_numeric_port() {
        assert_eq!(host_name("a.org:8080"), "a.org");
        assert_eq!(host_name("[::1]:8080"), "[::1]:8080");
        assert_eq!(host_name("a.org"), "a.org");
    }
}
