//! MediaWiki: every way to read a page, as the page's canonical article
//! URL.
//!
//! A title is canonical as MediaWiki makes it: `_` and whitespace runs are
//! one space, surrounding space is dropped, a `#section` is dropped, and on
//! a `capital_links` site the first letter is upper-cased. The page's
//! resource is the article URL of that title (`/wiki/Dead_drop`),
//! normalized like any URL, on the request's scheme and host (the mobile
//! host `en.m.wikipedia.org` folded into `en.wikipedia.org` when the site
//! says so). Namespaces are not folded (`talk:` and `Talk:` stay apart).

use a2a_bench_format::resource::Resource;
use url::form_urlencoded;

use crate::url::percent::decode;
use crate::url::{NormalUrl, normalize};

use super::MediaWikiSite;

const REST_PAGE: &str = "rest.php/v1/page/";
const WIKIMEDIA_REST_PAGE: &str = "/api/rest_v1/page/";

/// The pages a `GET` of `url` reads on `site`, in order; `None` when it is
/// not a page request this rule knows. Only the first is the URL's
/// resource; an empty list leaves the URL as it is.
pub(super) fn apply(site: &MediaWikiSite, url: &NormalUrl) -> Option<Vec<Resource>> {
    let host = if site.fold_mobile_host {
        fold_mobile(&url.host)
    } else {
        url.host.clone()
    };
    let page = |title: &str| page_resource(site, &url.scheme, &host, title);
    let params: Vec<(String, String)> = url
        .query
        .as_deref()
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect()
        })
        .unwrap_or_default();
    let param = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let path = url.path.as_str();
    let this_url = || Resource::Url(url.text());

    if let Some(title) = path.strip_prefix(site.article_path) {
        return Some(vec![page(&decode(title))?]);
    }
    if let Some(rest) = path.strip_prefix(WIKIMEDIA_REST_PAGE) {
        let (_endpoint, rest) = rest.split_once('/')?;
        let title = rest.split('/').next()?;
        return Some(vec![page(&decode(title))?]);
    }
    let script = path.strip_prefix(site.script_path)?;
    if let Some(rest) = script.strip_prefix(REST_PAGE) {
        let title = rest.split('/').next()?;
        return Some(vec![page(&decode(title))?]);
    }
    match script {
        "index.php" => Some(vec![page(param("title")?)?]),
        "api.php" => match param("action")? {
            "edit" | "delete" | "protect" | "undelete" => Some(vec![page(param("title")?)?]),
            "move" => Some(vec![page(param("from")?)?, page(param("to")?)?]),
            "query" => match param("titles") {
                Some(titles) => Some(titles.split('|').filter_map(page).collect()),
                None => Some(vec![this_url()]),
            },
            "parse" => match param("page") {
                Some(title) => Some(vec![page(title)?]),
                None => Some(vec![this_url()]),
            },
            _ => Some(vec![this_url()]),
        },
        _ => None,
    }
}

/// `raw` as MediaWiki canonicalizes a title, `None` when nothing is left.
fn canonical_title(raw: &str, capital_links: bool) -> Option<String> {
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
fn page_resource(site: &MediaWikiSite, scheme: &str, host: &str, title: &str) -> Option<Resource> {
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
    normalize(&format!("{scheme}://{host}{path}"))
        .ok()
        .map(crate::url::Normalized::into_resource)
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

#[cfg(test)]
mod tests {
    use super::{canonical_title, fold_mobile};

    #[test]
    fn titles_fold_separators_and_the_first_letter() {
        assert_eq!(
            canonical_title(" dead__drop #x", true).as_deref(),
            Some("Dead drop")
        );
        assert_eq!(
            canonical_title("dead_drop", false).as_deref(),
            Some("dead drop")
        );
        assert_eq!(canonical_title("ßx", true).as_deref(), Some("SSx"));
        assert_eq!(canonical_title(" _ #a", true), None);
    }

    #[test]
    fn mobile_hosts_fold() {
        assert_eq!(fold_mobile("en.m.wikipedia.org"), "en.wikipedia.org");
        assert_eq!(fold_mobile("m.wikidata.org"), "wikidata.org");
        assert_eq!(fold_mobile("m.org"), "m.org");
        assert_eq!(fold_mobile("en.m.org"), "en.org");
    }
}
