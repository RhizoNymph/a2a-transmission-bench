//! The route an injection takes into the victim: the tool call that read it.
//!
//! - `get_webpage(url)` reads a web page: a channel through the URL.
//!   AgentDojo strips the scheme before looking a page up, so a scheme-less
//!   URL is the same page; it is read as `http://`, the scheme the dataset's
//!   calls almost always use.
//! - `read_file(file_path)` reads the banking suite's flat file system: a
//!   channel through the file, rooted at `/`.
//! - Every other tool is keyed by something no canonical resource names (an
//!   email, an event, a channel name, a review), so the injection arrives
//!   `Direct` in the tool result.
//!
//! The URL is written as crosstalk-eval wrote it (its own small parser,
//! [`parse_url`], and `locator_key`), so the label's text is ct-eval's; the
//! scorer canonicalises every resource again before comparing.

use a2a_bench_format::labels::Route;
use a2a_bench_format::resource::Resource;

use crate::schema::RawCall;

/// The expected route of content read by `call`.
pub fn expected_route(call: Option<&RawCall>) -> Route {
    let Some(call) = call else {
        return Route::Direct;
    };
    let argument = |name: &str| call.args.get(name).and_then(|value| value.as_str());
    let resource = match call.function.as_str() {
        "get_webpage" => argument("url").and_then(url),
        "read_file" => argument("file_path")
            .filter(|path| !path.is_empty())
            .map(|path| Resource::File {
                path: normalize_path(&format!("/{path}")),
            }),
        _ => None,
    };
    match resource {
        Some(resource) => Route::Channel { resource },
        None => Route::Direct,
    }
}

fn url(text: &str) -> Option<Resource> {
    let text = if text.contains("://") {
        parse_url(text)
    } else {
        parse_url(&format!("http://{text}"))
    };
    text.map(Resource::Url)
}

/// crosstalk-eval's `reference::route::parse_url`, written as its
/// `locator_key`: an `http`/`https` URL with the scheme and host lower
/// case, a default port dropped, the fragment dropped, an empty path `/`,
/// and the query's non-empty parameters sorted. `None` for another
/// scheme, whitespace, or no host.
pub fn parse_url(text: &str) -> Option<String> {
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    if rest.chars().any(char::is_whitespace) {
        return None;
    }
    let rest = rest.split('#').next().unwrap_or(rest);
    let (authority, path_and_query) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    let host = authority.to_ascii_lowercase();
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("http", Some((name, "80"))) | ("https", Some((name, "443"))) => name.to_owned(),
        _ => host,
    };
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => {
            let mut params: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
            params.sort_unstable();
            (path, (!params.is_empty()).then(|| params.join("&")))
        }
        None => (path_and_query, None),
    };
    Some(match query {
        Some(query) => format!("{scheme}://{host}{path}?{query}"),
        None => format!("{scheme}://{host}{path}"),
    })
}

/// An absolute path with `.` and `..` resolved and repeated slashes merged
/// (crosstalk-eval's `normalize_path`).
pub fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}
