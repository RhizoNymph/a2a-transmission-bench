//! Shared resources a channel transmission goes through. These are shapes
//! only; `a2a-bench-resource` canonicalises them, and the scorer compares
//! canonical forms.

use serde::{Deserialize, Serialize};

/// A forge repository: host, owner (`/`-separated for nested groups) and name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub host: String,
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    /// A GitHub issue or pull request (one number space), or a GitLab issue.
    Issue,
    MergeRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    Issues,
    Pulls,
    MergeRequests,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Resource {
    Repository(Repository),
    RepoFile {
        repository: Repository,
        path: String,
    },
    Thread {
        repository: Repository,
        kind: ThreadKind,
        number: u64,
    },
    Collection {
        repository: Repository,
        kind: CollectionKind,
    },
    Url(String),
    /// A file outside any repository: on a filesystem the agents share, or
    /// on `host` when the file is on a named machine.
    File {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        path: String,
    },
    /// A resource behind an MCP tool, keyed by the argument that names it.
    Mcp {
        server: String,
        tool: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    /// A resource behind a tool, keyed by the argument that names it.
    Opaque {
        tool: String,
        key: String,
    },
}
