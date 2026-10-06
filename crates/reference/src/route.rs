//! Carrier and route of a hit, from where it sits in the reader's input.
//!
//! - A user turn is `Direct` with carrier `user_turn`, a system prompt
//!   `Direct` with carrier `system_prompt`.
//! - A tool result is a `Channel` when the call that produced it names a
//!   resource the reference can extract (an `http(s)` URL, or an absolute
//!   file path in a path-like argument), and `Direct` otherwise. A relative
//!   path is not extracted: with no known working directory it is not a
//!   canonical resource.
//!
//! The reference only ever names [`Resource::Url`] and [`Resource::File`];
//! the bench canonicalises them again before comparing.

use a2a_bench_format::check::WorldInputs;
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::message::{AssistantPart, Body, ToolArguments, ToolCall};
use a2a_bench_format::resource::Resource;
use serde_json::Value;

/// Argument names whose string value is a file path.
const PATH_KEYS: &[&str] = &[
    "path",
    "file",
    "file_path",
    "filepath",
    "filename",
    "target",
    "source_path",
    "target_path",
];

/// An `http(s)` URL, normalised: scheme and host lower-cased, the default
/// port and the fragment dropped, query parameters sorted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub path: String,
    pub query: Option<String>,
}

impl Url {
    /// The URL as text: `scheme://host/path[?query]`.
    pub fn key(&self) -> String {
        match &self.query {
            Some(query) => format!("{}://{}{}?{query}", self.scheme, self.host, self.path),
            None => format!("{}://{}{}", self.scheme, self.host, self.path),
        }
    }
}

/// A resource the reference routes a channel through.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChannelResource {
    Url(Url),
    /// An absolute path, normalised ([`normalize_path`]).
    File {
        path: String,
    },
}

impl ChannelResource {
    /// The channel's key: the URL's text, or `file://<path>`. Hits through
    /// one key share a channel.
    pub fn key(&self) -> String {
        match self {
            Self::Url(url) => url.key(),
            Self::File { path } => format!("file://{path}"),
        }
    }

    /// The bench resource it names.
    pub fn resource(&self) -> Resource {
        match self {
            Self::Url(url) => Resource::Url(url.key()),
            Self::File { path } => Resource::File { path: path.clone() },
        }
    }
}

/// The tool call with id `call` among `exchange`'s request messages: the
/// last one, when several share the id.
pub fn find_call<'a>(
    inputs: &'a WorldInputs,
    exchange: &Exchange,
    call: &str,
) -> Option<&'a ToolCall> {
    let mut found = None;
    for id in &exchange.request.messages {
        let Some(message) = inputs.message(*id) else {
            continue;
        };
        if let Body::Assistant(parts) = message.body() {
            for part in parts {
                if let AssistantPart::ToolCall(tool_call) = part
                    && tool_call.call_id == call
                {
                    found = Some(tool_call);
                }
            }
        }
    }
    found
}

/// The resource a tool call's arguments name, if the reference can extract
/// one: the first string member, in member-name order, that is a URL, or
/// that is an absolute path under a path-like name.
pub fn extract_resource(call: &ToolCall) -> Option<ChannelResource> {
    let ToolArguments::Json(json) = &call.arguments else {
        return None;
    };
    let value: Value = serde_json::from_str(json.as_str()).ok()?;
    let Value::Object(members) = value else {
        return None;
    };
    // Members are sorted (canonical JSON), so the choice is deterministic.
    for (name, member) in &members {
        let Value::String(text) = member else {
            continue;
        };
        if let Some(url) = parse_url(text) {
            return Some(ChannelResource::Url(url));
        }
        if PATH_KEYS.contains(&name.as_str()) && text.starts_with('/') {
            return Some(ChannelResource::File {
                path: normalize_path(text),
            });
        }
    }
    None
}

/// A URL with scheme and host lowercased, the fragment dropped and query
/// parameters sorted; `None` unless it is `http` or `https` with a host and
/// no whitespace.
pub fn parse_url(text: &str) -> Option<Url> {
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
    Some(Url {
        scheme,
        host,
        path: path.to_owned(),
        query,
    })
}

/// An absolute path with `.` and `..` resolved and repeated slashes merged.
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
