//! The spec table's notation for resources, and crosstalk's locators in the
//! same notation.
//!
//! `repository <host>/<owner>/<name>`, `repo_file <repo> <path>`,
//! `thread <repo> <kind> <N>`, `collection <repo> <kind>`, `url <text>`,
//! `file <path>`, `opaque <tool> <key>`.

#![allow(dead_code, clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use a2a_bench_format::resource::{CollectionKind, Repository, Resource, ThreadKind};
use serde_json::Value;

pub const VECTORS: &str = include_str!("../fixtures/crosstalk-7f8a2fb-resource-vectors.json");

pub fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("the vectors are JSON")
}

pub fn repo_text(repo: &Repository) -> String {
    format!("{}/{}/{}", repo.host, repo.owner, repo.name)
}

/// `resource` in the spec notation.
pub fn render(resource: &Resource) -> String {
    match resource {
        Resource::Repository(repo) => format!("repository {}", repo_text(repo)),
        Resource::RepoFile { repository, path } => {
            format!("repo_file {} {path}", repo_text(repository))
        }
        Resource::Thread {
            repository,
            kind,
            number,
        } => {
            let kind = match kind {
                ThreadKind::Issue => "issue",
                ThreadKind::MergeRequest => "merge_request",
            };
            format!("thread {} {kind} {number}", repo_text(repository))
        }
        Resource::Collection { repository, kind } => {
            format!(
                "collection {} {}",
                repo_text(repository),
                collection_kind(*kind)
            )
        }
        Resource::Url(text) => format!("url {text}"),
        Resource::File { host: None, path } => format!("file {path}"),
        Resource::File {
            host: Some(host),
            path,
        } => format!("file @{host} {path}"),
        Resource::Mcp {
            server,
            tool,
            target: None,
        } => format!("mcp {server} {tool}"),
        Resource::Mcp {
            server,
            tool,
            target: Some(target),
        } => format!("mcp {server} {tool} {target}"),
        Resource::Opaque { tool, key } => format!("opaque {tool} {key}"),
    }
}

fn collection_kind(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Issues => "issues",
        CollectionKind::Pulls => "pulls",
        CollectionKind::MergeRequests => "merge_requests",
    }
}

fn parse_repo(text: &str) -> Repository {
    let (host, rest) = text.split_once('/').expect("host/owner/name");
    let (owner, name) = rest.rsplit_once('/').expect("owner/name");
    Repository {
        host: host.to_owned(),
        owner: owner.to_owned(),
        name: name.to_owned(),
    }
}

/// A resource written in the spec notation.
pub fn parse(text: &str) -> Resource {
    let (tag, rest) = text.split_once(' ').expect("a tag and its parts");
    let mut words = rest.splitn(2, ' ');
    let mut word = || words.next().expect("another part").to_owned();
    match tag {
        "repository" => Resource::Repository(parse_repo(rest)),
        "repo_file" => Resource::RepoFile {
            repository: parse_repo(&word()),
            path: word(),
        },
        "thread" => {
            let repository = parse_repo(&word());
            let rest = word();
            let (kind, number) = rest.split_once(' ').expect("kind and number");
            let kind = match kind {
                "issue" => ThreadKind::Issue,
                "merge_request" => ThreadKind::MergeRequest,
                other => panic!("thread kind {other}"),
            };
            Resource::Thread {
                repository,
                kind,
                number: number.parse().expect("a number"),
            }
        }
        "collection" => {
            let repository = parse_repo(&word());
            let kind = match word().as_str() {
                "issues" => CollectionKind::Issues,
                "pulls" => CollectionKind::Pulls,
                "merge_requests" => CollectionKind::MergeRequests,
                other => panic!("collection kind {other}"),
            };
            Resource::Collection { repository, kind }
        }
        "url" => Resource::Url(rest.to_owned()),
        "file" => match rest.strip_prefix('@') {
            Some(hosted) => {
                let (host, path) = hosted.split_once(' ').expect("host and path");
                Resource::File {
                    host: Some(host.to_owned()),
                    path: path.to_owned(),
                }
            }
            None => Resource::File {
                host: None,
                path: rest.to_owned(),
            },
        },
        "mcp" => {
            let mut parts = rest.splitn(3, ' ');
            let mut part = || parts.next().map(str::to_owned);
            Resource::Mcp {
                server: part().expect("a server"),
                tool: part().expect("a tool"),
                target: part(),
            }
        }
        "opaque" => Resource::Opaque {
            tool: word(),
            key: word(),
        },
        other => panic!("resource tag {other}"),
    }
}

/// `resource` as crosstalk's locator for it would be rendered: a thread or
/// collection as its canonical page URL (GitLab's style on `gitlab.com`,
/// GitHub's elsewhere), everything else as [`render`].
pub fn render_as_locator(resource: &Resource) -> String {
    let page = |repo: &Repository, page: String| {
        format!(
            "url https://{}/{}/{}/{page}",
            repo.host, repo.owner, repo.name
        )
    };
    match resource {
        Resource::Thread {
            repository,
            kind,
            number,
        } => {
            let gitlab = repository.host == "gitlab.com";
            let path = match (gitlab, kind) {
                (false, _) => format!("issues/{number}"),
                (true, ThreadKind::MergeRequest) => format!("-/merge_requests/{number}"),
                (true, ThreadKind::Issue) => format!("-/issues/{number}"),
            };
            page(repository, path)
        }
        Resource::Collection { repository, kind } => {
            let gitlab = repository.host == "gitlab.com";
            let path = match (gitlab, kind) {
                (false, CollectionKind::Issues) => "issues",
                (false, CollectionKind::Pulls | CollectionKind::MergeRequests) => "pulls",
                (true, CollectionKind::Issues) => "-/issues",
                (true, CollectionKind::Pulls | CollectionKind::MergeRequests) => "-/merge_requests",
            };
            page(repository, path.to_owned())
        }
        other => render(other),
    }
}

/// A crosstalk `Locator` (its serde JSON) in the spec notation.
pub fn render_locator(locator: &Value) -> String {
    let data = &locator["data"];
    let text = |key: &str| data[key].as_str().expect("a string field").to_owned();
    match locator["type"].as_str().expect("a locator type") {
        "repository" => format!(
            "repository {}/{}/{}",
            text("host"),
            text("owner"),
            text("name")
        ),
        "file" => match data["host"].as_str() {
            Some(host) => format!("repo_file {host} {}", text("path")),
            None => format!("file {}", text("path")),
        },
        "url" => {
            let mut url = format!("url {}://{}{}", text("scheme"), text("host"), text("path"));
            if let Some(query) = data["query"].as_str() {
                url.push('?');
                url.push_str(query);
            }
            url
        }
        "opaque" => format!("opaque {} {}", text("tool"), text("key")),
        other => panic!("locator type {other}"),
    }
}
