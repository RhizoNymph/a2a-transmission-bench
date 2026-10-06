//! GitHub: the repository, its files and its issues and pull requests,
//! whatever GitHub URL reaches them.
//!
//! | URL | Resource |
//! | --- | --- |
//! | `github.com/<o>/<n>(.git)`, `…/tree/<ref>`, `codeload.github.com/<o>/<n>/…`, `api.github.com/repos/<o>/<n>` and its other subpaths | the repository |
//! | `github.com/<o>/<n>/blob\|raw/<ref>/<path>`, `raw.githubusercontent.com/<o>/<n>/(refs/heads\|tags/)<ref>/<path>`, `api.github.com/repos/<o>/<n>/contents/<path>` | the repository's file |
//! | `github.com/<o>/<n>/issues\|pull/<N>/…`, `api.github.com/repos/<o>/<n>/issues\|pulls/<N>/…` | the thread `issue <N>`: issues and pull requests share one number space and one conversation |
//! | `github.com/<o>/<n>/issues\|pulls`, `api.github.com/repos/<o>/<n>/issues\|pulls` | the collection |
//! | `<o>.github.io/<n>/…` (Pages) | the repository `<o>/<n>`; `<o>.github.io/` with no path is `<o>/<o>.github.io` |
//!
//! The ref is dropped, and the first segment after `blob`/`raw` is taken as
//! the ref even when the ref has a slash. A Pages path's first segment is
//! taken as the repository's name.

use a2a_bench_format::resource::{CollectionKind, Repository, Resource, ThreadKind};

use crate::repository::{forge, path::absolute};
use crate::url::NormalUrl;
use crate::url::percent::segments;

const HOST: &str = "github.com";

/// First path segments of `github.com` that are not an owner.
const NOT_OWNERS: [&str; 22] = [
    "about",
    "apps",
    "collections",
    "contact",
    "customer-stories",
    "enterprise",
    "events",
    "explore",
    "features",
    "issues",
    "login",
    "marketplace",
    "new",
    "notifications",
    "orgs",
    "pricing",
    "pulls",
    "search",
    "settings",
    "sponsors",
    "topics",
    "users",
];

pub(super) fn apply(url: &NormalUrl) -> Option<Resource> {
    let segments = segments(&url.path);
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (url.host.as_str(), segments.as_slice()) {
        ("github.com" | "www.github.com", [owner, ..]) if NOT_OWNERS.contains(owner) => None,
        ("github.com" | "www.github.com", [owner, repo, "blob" | "raw", _ref, file @ ..]) => {
            file_resource(owner, repo, file)
        }
        ("github.com" | "www.github.com", [owner, repo, "issues" | "pull", number, ..]) => {
            thread(owner, repo, number)
        }
        ("github.com" | "www.github.com", [owner, repo, page @ ("issues" | "pulls")]) => {
            collection(owner, repo, page)
        }
        ("github.com" | "www.github.com", [owner, repo])
        | ("github.com" | "www.github.com", [owner, repo, "tree", _])
        | ("codeload.github.com", [owner, repo, ..]) => repository(owner, repo),
        ("raw.githubusercontent.com", [owner, repo, "refs", "heads" | "tags", _ref, file @ ..])
        | ("raw.githubusercontent.com", [owner, repo, _ref, file @ ..]) => {
            file_resource(owner, repo, file)
        }
        ("api.github.com", ["repos", owner, repo, "contents", file @ ..]) => {
            file_resource(owner, repo, file)
        }
        ("api.github.com", ["repos", owner, repo, "issues" | "pulls", number, ..])
            if number.bytes().all(|b| b.is_ascii_digit()) =>
        {
            thread(owner, repo, number)
        }
        ("api.github.com", ["repos", owner, repo, page @ ("issues" | "pulls")]) => {
            collection(owner, repo, page)
        }
        ("api.github.com", ["repos", owner, repo, ..]) => repository(owner, repo),
        (pages, rest) => {
            let user = pages.strip_suffix(".github.io")?;
            if user.is_empty() || user.contains('.') {
                return None;
            }
            match rest {
                [] => repository(user, pages),
                [repo, ..] => repository(user, repo),
            }
        }
    }
}

fn repo(owner: &str, repo: &str) -> Option<Repository> {
    forge(HOST, &format!("{owner}/{repo}"))
}

fn repository(owner: &str, name: &str) -> Option<Resource> {
    repo(owner, name).map(Resource::Repository)
}

fn file_resource(owner: &str, name: &str, file: &[&str]) -> Option<Resource> {
    if file.is_empty() {
        return None;
    }
    let repository = repo(owner, name)?;
    let path = absolute(&format!("/{}", file.join("/")))?;
    Some(Resource::RepoFile { repository, path })
}

fn thread(owner: &str, name: &str, number: &str) -> Option<Resource> {
    let number: u64 = number.parse().ok()?;
    Some(Resource::Thread {
        repository: repo(owner, name)?,
        kind: ThreadKind::Issue,
        number,
    })
}

fn collection(owner: &str, name: &str, page: &str) -> Option<Resource> {
    let kind = if page == "issues" {
        CollectionKind::Issues
    } else {
        CollectionKind::Pulls
    };
    Some(Resource::Collection {
        repository: repo(owner, name)?,
        kind,
    })
}
