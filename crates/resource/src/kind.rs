//! Which resources are shared, and as what.

use a2a_bench_format::resource::Resource;

/// What kind of shared resource a resource is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceKind {
    /// A forge repository itself: what `git push`, `pull`, `fetch` and
    /// `clone` touch.
    Repository,
    /// A file of a forge repository.
    RepoFile,
    /// A web page: an issue or change thread or collection, an API URL,
    /// any other site.
    Url,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::RepoFile => "repo_file",
            Self::Url => "url",
        }
    }
}

/// The kind of a shared resource; `None` for a file (on one agent's own
/// filesystem or a named host), an MCP tool's resource or an opaque key (a
/// tool's resource, an invalid-host URL).
pub fn kind(resource: &Resource) -> Option<ResourceKind> {
    match resource {
        Resource::Repository(_) => Some(ResourceKind::Repository),
        Resource::RepoFile { .. } => Some(ResourceKind::RepoFile),
        Resource::Thread { .. } | Resource::Collection { .. } | Resource::Url(_) => {
            Some(ResourceKind::Url)
        }
        Resource::File { .. } | Resource::Mcp { .. } | Resource::Opaque { .. } => None,
    }
}
