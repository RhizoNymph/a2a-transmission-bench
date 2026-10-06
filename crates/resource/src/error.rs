//! Why text names no resource.

/// Why text is not a URL a resource can be made from.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("not a URL: {0}")]
    Parse(#[from] url::ParseError),
    #[error("the URL has no host")]
    NoHost,
}

/// Why `repository(host, owner, name)` refused its parts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRepository {
    /// Empty, or not a DNS name (ASCII letters, digits, `.`, `-`).
    #[error("`{0}` is not a repository host")]
    Host(String),
    /// Empty, or a segment that is empty, `.`, `..` or has a character
    /// other than ASCII letters, digits, `-`, `_` and `.`.
    #[error("`{0}` is not a repository owner")]
    Owner(String),
    /// Empty once `.git` is stripped, or has such a character or a `/`.
    #[error("`{0}` is not a repository name")]
    Name(String),
}

/// Why text is not a forge repository's git remote.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoteError {
    /// Not a remote in any form `canonical_remote` reads.
    #[error("not a git remote")]
    NotARemote,
    /// A repository on the local filesystem (`file://`, a path), which no
    /// other agent shares.
    #[error("a local repository, not a forge's")]
    Local,
    /// A remote path with no `owner/` before the name.
    #[error("the remote path `{0}` has no owner")]
    NoOwner(String),
    /// A forge remote whose parts are not a repository.
    #[error(transparent)]
    Repository(#[from] InvalidRepository),
}
