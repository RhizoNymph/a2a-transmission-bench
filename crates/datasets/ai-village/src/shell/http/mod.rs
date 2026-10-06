//! HTTP requests a command makes (`curl`, `wget`, `gh api`, `glab api`):
//! the method, the URL and the form fields, and the accesses they imply.
//!
//! A site rule ([`sites`]) that recognizes the request decides its
//! accesses (a forge repository, file or thread; a wiki page). Otherwise a
//! writing method (`POST`, `PUT`, `PATCH`, `DELETE`) writes the URL, a
//! reading one (`GET`, `HEAD`) reads it when the response body reaches the
//! output, and any other method is no access.

pub mod mediawiki;
pub mod sites;

use super::locator::Loc;
use super::outcome::CommandRule;
use super::{Candidate, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    /// Any other method: neither reads nor writes are assumed.
    Other,
}

impl Method {
    pub fn parse(text: &str) -> Self {
        match text.to_ascii_uppercase().as_str() {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            _ => Self::Other,
        }
    }

    pub fn writes(self) -> bool {
        matches!(self, Self::Post | Self::Put | Self::Patch | Self::Delete)
    }

    pub fn reads(self) -> bool {
        matches!(self, Self::Get | Self::Head)
    }
}

/// One request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// A canonical URL (or the `Opaque` of an invalid host).
    pub url: Loc,
    pub method: Method,
    /// Form fields sent in the body, decoded (`-d a=1&b=2`, `-F a=1`).
    pub form: Vec<(String, String)>,
}

impl HttpRequest {
    /// The query's parameters followed by the form's, decoded.
    pub fn params(&self) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = match &self.url {
            Loc::Url {
                query: Some(query), ..
            } => form_fields(query),
            _ => Vec::new(),
        };
        params.extend(self.form.iter().cloned());
        params
    }
}

/// Form fields of a URL-encoded body (`a=1&b=x+y`), decoded.
pub fn form_fields(body: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// The accesses of `request`, judged by `rule`. `body_to_result`: the
/// response body reaches the output (it is not saved to a file).
pub fn candidates(
    request: &HttpRequest,
    body_to_result: bool,
    rule: CommandRule,
    found: &mut Vec<Candidate>,
) {
    let (kind, locators) = match sites::apply(request) {
        Some(site) => (site.kind, site.locators),
        None if request.method.writes() => (Kind::Write, vec![request.url.clone()]),
        None if request.method.reads() => (Kind::Read, vec![request.url.clone()]),
        None => return,
    };
    match kind {
        Kind::Write => found.extend(
            locators
                .into_iter()
                .map(|locator| Candidate::write(locator).judged_by(rule)),
        ),
        Kind::Read if body_to_result => found.extend(
            locators
                .into_iter()
                .map(|locator| Candidate::read(locator).judged_by(rule)),
        ),
        Kind::Read => {}
    }
}
