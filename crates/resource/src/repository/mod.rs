//! Forge repositories in canonical form, and their files' paths.
//!
//! A repository is `{host, owner, name}`: `host` lower case without `www.`,
//! port or trailing dot; `owner` its lower-case namespace, `/`-separated
//! for nested groups (`group/subgroup`); `name` lower case without `.git`.
//! Forges treat these case-insensitively, so two spellings that differ
//! only in them name one repository.

pub(crate) mod path;
pub(crate) mod remote;

use a2a_bench_format::resource::Repository;

use crate::error::InvalidRepository;

/// The canonical repository `owner/name` on `host`. Folds host and path
/// case, one `www.`, a port, trailing dots on the host, `.git` and slashes
/// around the parts.
pub fn repository(host: &str, owner: &str, name: &str) -> Result<Repository, InvalidRepository> {
    let canonical_host =
        canonical_host(host).ok_or_else(|| InvalidRepository::Host(host.to_owned()))?;
    let segments: Vec<String> = owner
        .trim_matches('/')
        .split('/')
        .map(str::to_ascii_lowercase)
        .collect();
    if !segments.iter().all(|segment| valid_segment(segment)) {
        return Err(InvalidRepository::Owner(owner.to_owned()));
    }
    let trimmed = name.trim_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let canonical_name = trimmed.to_ascii_lowercase();
    if !valid_segment(&canonical_name) {
        return Err(InvalidRepository::Name(name.to_owned()));
    }
    Ok(Repository {
        host: canonical_host,
        owner: segments.join("/"),
        name: canonical_name,
    })
}

/// The repository at `path` (`owner/name`, or `group/subgroup/name`) on
/// `host`. `path` may carry `.git` and slashes around it; `None` when it
/// has no `/` between owner and name or the parts are not a repository.
pub(crate) fn forge(host: &str, path: &str) -> Option<Repository> {
    remote::forge(host, path).ok()
}

/// `host` in canonical form: up to the first `:`, trailing dots dropped,
/// lower case, one leading `www.` dropped. `None` when it is not a DNS
/// name.
fn canonical_host(host: &str) -> Option<String> {
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let valid = !host.is_empty()
        && !host.starts_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    valid.then(|| host.to_owned())
}

/// One `/`-separated segment of a repository's owner, or its name.
fn valid_segment(text: &str) -> bool {
    !text.is_empty()
        && text != "."
        && text != ".."
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
