//! Canonical resources: from text (a URL, a git remote) and from any
//! `Resource`.

use a2a_bench_format::resource::{Repository, Resource, ThreadKind};

use crate::error::UrlError;
use crate::repository::path::absolute;
use crate::repository::repository;
use crate::sites;
use crate::url::{Normalized, normalize, normalize_tool};

/// How many canonicalisation steps [`canonicalize`] takes at most. One
/// step suffices for every resource but a repository that [`repository`]
/// folds only part of the way: a host with several `www.` prefixes (one
/// goes per step), a name ending in an upper-case `.GIT` (lower-cased after
/// the suffix check) or in `.git.git`.
const MAX_STEPS: usize = 16;

/// The resource a `GET` of `text` reaches: the URL in canonical form
/// (`https://` assumed when `text` names a host without a scheme), then
/// what a site rule makes of it (a forge repository, file, thread or
/// collection; a wiki's article URL), else the URL itself. A `scheme://`
/// URL whose host is invalid is `Opaque` on `"<url>"`.
pub fn canonical_url(text: &str) -> Result<Resource, UrlError> {
    Ok(match normalize_tool(text)? {
        Normalized::Url(url) => sites::apply(&url).unwrap_or_else(|| Resource::Url(url.text())),
        invalid @ Normalized::InvalidHost { .. } => invalid.into_resource(),
    })
}

/// `text` as a URL in canonical form, with no site rule and no assumed
/// scheme: a `Url`, or `Opaque` on `"<url>"` for an invalid host.
pub fn normalized_url(text: &str) -> Result<Resource, UrlError> {
    normalize(text).map(Normalized::into_resource)
}

/// `resource` in canonical form, so that two spellings of one resource are
/// equal. Idempotent: `canonicalize(&canonicalize(r)) == canonicalize(r)`.
///
/// - a repository (also inside a file, thread or collection): its parts
///   canonical ([`repository`]); parts that are no repository stay as
///   they are;
/// - a repository file's path: absolute (a relative one is taken from the
///   repository root) and resolved lexically;
/// - a thread: a `pull` is an `issue` (GitHub's pull requests share the
///   issues' numbers and conversation);
/// - a URL: [`canonical_url`] of its text, so a URL that names a forge
///   page becomes that repository, file, thread or collection; text that
///   is no URL stays as it is;
/// - a file: an absolute path resolved lexically;
/// - an opaque key: as it is.
pub fn canonicalize(resource: &Resource) -> Resource {
    let mut current = step(resource);
    for _ in 1..MAX_STEPS {
        let next = step(&current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn step(resource: &Resource) -> Resource {
    match resource {
        Resource::Repository(repo) => Resource::Repository(canonical_repository(repo)),
        Resource::RepoFile { repository, path } => Resource::RepoFile {
            repository: canonical_repository(repository),
            path: repo_path(path),
        },
        Resource::Thread {
            repository,
            kind,
            number,
        } => Resource::Thread {
            repository: canonical_repository(repository),
            kind: match kind {
                ThreadKind::Issue | ThreadKind::Pull => ThreadKind::Issue,
                ThreadKind::MergeRequest => ThreadKind::MergeRequest,
            },
            number: *number,
        },
        Resource::Collection { repository, kind } => Resource::Collection {
            repository: canonical_repository(repository),
            kind: *kind,
        },
        Resource::Url(text) => canonical_url(text).unwrap_or_else(|_| resource.clone()),
        Resource::File { path } => Resource::File {
            path: absolute(path).unwrap_or_else(|| path.clone()),
        },
        Resource::Opaque { .. } => resource.clone(),
    }
}

fn canonical_repository(repo: &Repository) -> Repository {
    repository(&repo.host, &repo.owner, &repo.name).unwrap_or_else(|_| repo.clone())
}

fn repo_path(path: &str) -> String {
    let rooted = if path.starts_with('/') {
        absolute(path)
    } else {
        absolute(&format!("/{path}"))
    };
    rooted.unwrap_or_else(|| path.to_owned())
}
