//! GitLab (`gitlab.com`): the project, its files and its issues and merge
//! requests, whatever GitLab URL reaches them.
//!
//! | URL | Resource |
//! | --- | --- |
//! | `gitlab.com/<path…>/<n>(.git)`, `gitlab.com/<path…>/<n>/-/<other>`, `gitlab.com/api/v4/projects/<url-encoded path>` and its other subpaths | the repository (owner `<path…>`) |
//! | `gitlab.com/<path…>/<n>/-/blob\|raw/<ref>/<file>`, `…/api/v4/projects/<p>/repository/files/<url-encoded file>(/raw)` | the repository's file |
//! | `…/-/issues/<N>`, `…/-/merge_requests/<N>`, `…/api/v4/projects/<p>/issues\|merge_requests/<N>/…` | the thread |
//! | `…/-/issues`, `…/-/merge_requests`, `…/api/v4/projects/<p>/issues\|merge_requests` | the collection |
//! | `<g>.gitlab.io/<p>/…` (Pages) | the repository `<g>/<p>` |
//!
//! A numeric project id (`/api/v4/projects/123`) names no path and stays a
//! URL; so does a unique Pages domain (`<name>-<6 hex>.gitlab.io`) and a
//! bare `<g>.gitlab.io/`.

use a2a_bench_format::resource::{CollectionKind, Repository, Resource, ThreadKind};

use crate::repository::{forge, path::absolute};
use crate::url::NormalUrl;
use crate::url::percent::segments;

const HOST: &str = "gitlab.com";

/// First path segments of `gitlab.com` that are not a namespace.
const NOT_NAMESPACES: [&str; 8] = [
    "-",
    "api",
    "dashboard",
    "explore",
    "groups",
    "help",
    "users",
    "search",
];

pub(super) fn apply(url: &NormalUrl) -> Option<Resource> {
    let segments = segments(&url.path);
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (url.host.as_str(), segments.as_slice()) {
        ("gitlab.com" | "www.gitlab.com", ["api", "v4", "projects", project, rest @ ..]) => {
            api(project_path(project)?, rest)
        }
        ("gitlab.com" | "www.gitlab.com", [first, ..]) if NOT_NAMESPACES.contains(first) => None,
        ("gitlab.com" | "www.gitlab.com", all) => {
            let (project, rest) = match all.iter().position(|segment| *segment == "-") {
                Some(at) => (all.get(..at)?, all.get(at + 1..)?),
                None => (all, &[][..]),
            };
            web(forge(HOST, &project.join("/"))?, rest)
        }
        (pages, rest) => {
            let group = pages.strip_suffix(".gitlab.io")?;
            if group.is_empty() || group.contains('.') || unique_domain(group) {
                return None;
            }
            let [project, ..] = rest else {
                return None;
            };
            forge(HOST, &format!("{group}/{project}")).map(Resource::Repository)
        }
    }
}

/// `<name>-<6 hex>`: a unique Pages domain, which names no project path.
fn unique_domain(label: &str) -> bool {
    label
        .rsplit_once('-')
        .is_some_and(|(_, hex)| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The project an API path's id names, when it is a URL-encoded path
/// (already decoded by the segment split); `None` for a numeric id.
fn project_path(project: &str) -> Option<Repository> {
    if project.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    forge(HOST, project)
}

fn web(repository: Repository, rest: &[&str]) -> Option<Resource> {
    match rest {
        ["blob" | "raw", _ref, file @ ..] => file_resource(repository, file),
        [page @ ("issues" | "merge_requests"), number, ..] => thread(repository, page, number),
        [page @ ("issues" | "merge_requests")] => Some(collection(repository, page)),
        _ => Some(Resource::Repository(repository)),
    }
}

fn api(repository: Repository, rest: &[&str]) -> Option<Resource> {
    match rest {
        ["repository", "files", file] | ["repository", "files", file, "raw"] => {
            file_resource(repository, &[file])
        }
        [page @ ("issues" | "merge_requests"), number, ..]
            if number.bytes().all(|b| b.is_ascii_digit()) =>
        {
            thread(repository, page, number)
        }
        [page @ ("issues" | "merge_requests")] => Some(collection(repository, page)),
        _ => Some(Resource::Repository(repository)),
    }
}

fn file_resource(repository: Repository, file: &[&str]) -> Option<Resource> {
    if file.is_empty() {
        return None;
    }
    let path = absolute(&format!("/{}", file.join("/")))?;
    Some(Resource::RepoFile { repository, path })
}

fn thread(repository: Repository, page: &str, number: &str) -> Option<Resource> {
    let number: u64 = number.parse().ok()?;
    let kind = if page == "issues" {
        ThreadKind::Issue
    } else {
        ThreadKind::MergeRequest
    };
    Some(Resource::Thread {
        repository,
        kind,
        number,
    })
}

fn collection(repository: Repository, page: &str) -> Resource {
    let kind = if page == "issues" {
        CollectionKind::Issues
    } else {
        CollectionKind::MergeRequests
    };
    Resource::Collection { repository, kind }
}
