//! URLs in canonical form, before any site rule.
//!
//! The [`url`] crate parses (WHATWG): it lowercases the scheme and a special
//! scheme's host (IDNA to punycode), drops the default port and resolves
//! `.`/`..` path segments. On top of that:
//! - the fragment and any user info are dropped;
//! - a non-special scheme's host is lowercased and a domain's trailing dot
//!   dropped;
//! - percent-encoding in the path and query is normalized ([`percent`]);
//! - the query's parameters are sorted, each keeping its text, and empty
//!   ones dropped; an empty query is no query.
//!
//! A `scheme://` URL whose host the parser refuses becomes an opaque key
//! ([`invalid_host`]); text with no scheme that names a host is read as
//! `https://` ([`bare`]).

mod bare;
mod invalid_host;
pub(crate) mod percent;

use a2a_bench_format::resource::Resource;
use url::Url;

use crate::error::UrlError;

pub use invalid_host::INVALID_HOST_TOOL;

/// A URL in canonical form: the parts of its canonical text
/// `{scheme}://{host}{path}[?{query}]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalUrl {
    pub scheme: String,
    /// Lower case, with `:port` when the port is not the scheme's default.
    pub host: String,
    /// Starts with `/`.
    pub path: String,
    /// Sorted parameters; never empty.
    pub query: Option<String>,
}

impl NormalUrl {
    pub fn text(&self) -> String {
        let mut text = format!("{}://{}{}", self.scheme, self.host, self.path);
        if let Some(query) = &self.query {
            text.push('?');
            text.push_str(query);
        }
        text
    }
}

/// What a URL's text normalizes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Normalized {
    Url(NormalUrl),
    /// A `scheme://` URL whose host is invalid, keyed by its normalized
    /// text.
    InvalidHost {
        key: String,
    },
}

impl Normalized {
    /// The resource itself: a `Url` of the canonical text, or the
    /// `Opaque` key of an invalid host on [`INVALID_HOST_TOOL`].
    pub(crate) fn into_resource(self) -> Resource {
        match self {
            Self::Url(url) => Resource::Url(url.text()),
            Self::InvalidHost { key } => Resource::Opaque {
                tool: INVALID_HOST_TOOL.to_owned(),
                key,
            },
        }
    }
}

/// `text` in canonical form, as written (no scheme is assumed).
pub(crate) fn normalize(text: &str) -> Result<Normalized, UrlError> {
    let text = text.trim();
    let url = match Url::parse(text) {
        Ok(url) => url,
        Err(error) => {
            return invalid_host::key(text, error)
                .map(|key| Normalized::InvalidHost { key })
                .ok_or(UrlError::Parse(error));
        }
    };
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or(UrlError::NoHost)?;
    let mut host = host.to_ascii_lowercase();
    if host.len() > 1 && host.ends_with('.') {
        host.pop();
    }
    if let Some(port) = url.port() {
        host = format!("{host}:{port}");
    }
    let path = match url.path() {
        "" => "/".to_owned(),
        path => percent::normalize(path),
    };
    Ok(Normalized::Url(NormalUrl {
        scheme: url.scheme().to_owned(),
        host,
        path,
        query: url.query().and_then(sorted_query),
    }))
}

/// [`normalize`], and when that fails on text with no scheme that names a
/// host (`www.example.com`, `example.com:8080/a`), `https://` + the text.
pub(crate) fn normalize_tool(text: &str) -> Result<Normalized, UrlError> {
    normalize(text).or_else(|error| match bare::host(text.trim()) {
        Some(bare) => normalize(&format!("https://{bare}")),
        None => Err(error),
    })
}

fn sorted_query(query: &str) -> Option<String> {
    let mut parameters: Vec<String> = query
        .split('&')
        .filter(|parameter| !parameter.is_empty())
        .map(percent::normalize)
        .collect();
    if parameters.is_empty() {
        return None;
    }
    parameters.sort();
    Some(parameters.join("&"))
}
