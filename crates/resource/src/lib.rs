//! The bench's neutral resource canonicaliser: the one definition of when a
//! label and a prediction name the same shared resource.
//!
//! Converters make labels' resources with [`canonical_url`] and
//! [`canonical_remote`]; the scorer applies [`canonicalize`] to labels and
//! predictions alike before comparing them. The normative definition is
//! the table in `docs/features/resource.md`; version 1 equals crosstalk's
//! extractor at commit 7f8a2fb.

mod canonical;
mod error;
mod kind;
mod repository;
mod sites;
mod url;

pub use canonical::{canonical_url, canonicalize, normalized_url};
pub use error::{InvalidRepository, RemoteError, UrlError};
pub use kind::{ResourceKind, kind};
pub use repository::remote::canonical_remote;
pub use repository::repository;
pub use url::INVALID_HOST_TOOL;
