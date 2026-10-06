//! The synthetic tool shape agents' reads and writes take, defined in one
//! place.
//!
//! HTTP-style, the page in the `url` argument so a read and a write of one
//! page name the same resource:
//!
//! - read: `http_request {"method":"GET","url":<page>}`, whose result is the
//!   page body;
//! - write: `http_request {"method":"POST","url":<page>,"body":<inserted text>}`.
//!
//! Never MCP-shaped: a tool-keyed resource would split a read and a write
//! of one page into two resources.

use serde_json::{Value, json};

/// The tool name both a read and a write use.
pub const TOOL: &str = "http_request";

/// The read call's arguments: a `GET` of the page URL.
pub fn read_args(url: &str) -> Value {
    json!({ "method": "GET", "url": url })
}

/// The write call's arguments: a `POST` to the page URL whose body is the
/// lines this revision inserted.
pub fn write_args(url: &str, body: &str) -> Value {
    json!({ "method": "POST", "url": url, "body": body })
}
