//! Git remotes: the forge repository a remote names, in every spelling git
//! accepts.

use a2a_bench_format::resource::Repository;

use crate::error::RemoteError;

use super::path::absolute;
use super::repository;

/// URL schemes of a network remote.
const SCHEMES: [&str; 6] = ["http", "https", "ssh", "git", "git+ssh", "ssh+git"];

/// The forge repository `remote` names: `http(s)://`, `ssh://`, `git://`,
/// `git+ssh://` or `ssh+git://[user@]host[:port]/owner/name(.git)`, or the
/// scp form `[user@]host:owner/name(.git)` whose host has a dot or is
/// `localhost`. The repository is the path's last segment, its owner every
/// segment before.
///
/// A local repository (`file://…`, an absolute path, `./…`) is
/// [`RemoteError::Local`]; anything else (text with whitespace, another
/// scheme, a drive letter) [`RemoteError::NotARemote`].
pub fn canonical_remote(remote: &str) -> Result<Repository, RemoteError> {
    let remote = remote.trim();
    if remote.is_empty() || remote.contains(char::is_whitespace) {
        return Err(RemoteError::NotARemote);
    }
    if let Some(path) = remote.strip_prefix("file://") {
        return Err(local(path));
    }
    if let Some((scheme, rest)) = remote.split_once("://") {
        if !SCHEMES.contains(&scheme) {
            return Err(RemoteError::NotARemote);
        }
        let (authority, path) = rest.split_once('/').ok_or(RemoteError::NotARemote)?;
        let host = authority.rsplit('@').next().unwrap_or(authority);
        let host = host.split(':').next().unwrap_or(host);
        return forge(host, path);
    }
    if remote.starts_with('/') {
        return Err(local(remote));
    }
    if let Some((authority, path)) = remote.split_once(':')
        && !authority.contains('/')
        && !path.starts_with("//")
    {
        let host = authority.rsplit('@').next().unwrap_or(authority);
        // A host has a dot (`github.com`) or is `localhost`; a drive letter
        // (`C:/x`) is not one.
        if host.contains('.') || host == "localhost" {
            return forge(host, path);
        }
        return Err(RemoteError::NotARemote);
    }
    if remote.starts_with('.') {
        return Err(RemoteError::Local);
    }
    Err(RemoteError::NotARemote)
}

/// A local path is never a forge remote; one that is not a path at all is
/// not a remote.
fn local(path: &str) -> RemoteError {
    match absolute(path) {
        Some(_) => RemoteError::Local,
        None => RemoteError::NotARemote,
    }
}

/// The repository at `path` on `host` ([`super::forge`]).
pub(super) fn forge(host: &str, path: &str) -> Result<Repository, RemoteError> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path
        .rsplit_once('/')
        .ok_or_else(|| RemoteError::NoOwner(path.to_owned()))?;
    Ok(repository(host, owner, name)?)
}
