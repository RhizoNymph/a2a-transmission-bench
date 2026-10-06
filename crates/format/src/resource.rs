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
    Issue,
    Pull,
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
    /// A file outside any repository, on a filesystem the agents share.
    File {
        path: String,
    },
    /// A resource behind a tool, keyed by the argument that names it.
    Opaque {
        tool: String,
        key: String,
    },
}
