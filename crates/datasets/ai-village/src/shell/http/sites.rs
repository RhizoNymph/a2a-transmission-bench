//! Site rules: the pages of known forges and wikis get one locator,
//! whatever URL or method reaches them.
//!
//! What a `GET` of a URL reaches is `a2a_bench_resource::canonical_url`'s
//! (the bench's resource identity). A request with another method keeps
//! that resource only where the site's page takes the method:
//!
//! | Site | URL | `GET`/`HEAD` | a writing method | another method |
//! | --- | --- | --- | --- | --- |
//! | MediaWiki (`*.wikipedia.org` … `*.fandom.com`) | see [`super::mediawiki`] (the form's fields count as parameters) | | | |
//! | GitHub API (`api.github.com/repos/o/n/…`), GitLab API (`gitlab.com/api/v4/projects/<path>/…`) | the repository, file, thread or collection | read | write | no access |
//! | GitHub `blob`/`raw` file pages, `raw.githubusercontent.com` | the repository file | read | read | read |
//! | every other forge page (web, `codeload`, Pages) | the repository, thread or collection | read | no site rule | no site rule |
//!
//! "No site rule" leaves the request to the plain URL rule: a writing
//! method writes the URL itself. A URL that names no forge or wiki page
//! has no site rule.

use a2a_bench_format::resource::Resource;
use a2a_bench_resource::canonical_url;

use super::super::Kind;
use super::super::locator::Loc;
use super::{HttpRequest, mediawiki};

/// What a site rule decided for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteAccess {
    pub kind: Kind,
    pub locators: Vec<Loc>,
}

/// The rule that recognizes `request`, if any.
pub fn apply(request: &HttpRequest) -> Option<SiteAccess> {
    let Loc::Url { host, path, .. } = &request.url else {
        return None;
    };
    let name = host_name(host);
    if let Some(sites) = mediawiki::sites_of(name) {
        return sites
            .iter()
            .find_map(|site| mediawiki::apply(site, request));
    }
    let text = request.url.url_text()?;
    let resource = match canonical_url(&text).ok()? {
        Resource::Url(_) | Resource::Opaque { .. } | Resource::File { .. } => return None,
        resource => resource,
    };
    let locator = Loc::of_site(&resource)?;
    let segments = segments(path);
    let first = segments.first().map(String::as_str);
    let api = host == "api.github.com"
        || (matches!(host.as_str(), "gitlab.com" | "www.gitlab.com") && first == Some("api"));
    let file_page = matches!(resource, Resource::RepoFile { .. })
        && (host == "raw.githubusercontent.com"
            || (matches!(host.as_str(), "github.com" | "www.github.com")
                && matches!(segments.get(2).map(String::as_str), Some("blob" | "raw"))));
    let kind = if file_page {
        Kind::Read
    } else if api {
        if request.method.reads() {
            Kind::Read
        } else if request.method.writes() {
            Kind::Write
        } else {
            return None;
        }
    } else if request.method.reads() {
        Kind::Read
    } else {
        return None;
    };
    Some(SiteAccess {
        kind,
        locators: vec![locator],
    })
}

/// A host without its port.
pub fn host_name(host: &str) -> &str {
    if host.starts_with('[') {
        return host;
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

/// A URL path's non-empty segments, each percent-decoded.
pub fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .collect()
}

/// `text` with every `%XX` escape decoded; an undecodable result keeps the
/// text as it was.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |at: usize| {
            bytes
                .get(at)
                .and_then(|b| char::from(*b).to_digit(16))
                .and_then(|d| u8::try_from(d).ok())
        };
        match (bytes[index], hex(index + 1), hex(index + 2)) {
            (b'%', Some(high), Some(low)) => {
                out.push(high * 16 + low);
                index += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}
