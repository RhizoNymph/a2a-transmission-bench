//! What a shell access names: the shell model's locators, the clones it
//! knows, and how a locator becomes a bench [`Resource`].
//!
//! A [`Loc`] has the shapes of the resources a bash command can touch (a
//! URL, a file on a host or in a repository, a forge repository, a key that
//! is no URL). Every URL and repository in one comes from
//! `a2a-bench-resource`, so a label's resource and a detector's prediction
//! meet on the bench's canonical form. Only shared resources leave the
//! shell ([`Loc::kind`]); [`Loc::resource`] turns one into a [`Resource`].
//!
//! Directories are [`AbsolutePath`]s, resolved lexically; a clone is a
//! directory bound to a [`RepoId`] ([`RepoBindings`]).

use std::fmt;

use a2a_bench_format::resource::{CollectionKind, Repository, Resource, ThreadKind};
use a2a_bench_resource::{
    ResourceKind, canonical_remote, canonical_url, normalized_url, repository,
};

/// One thing a command reads or writes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Loc {
    /// A URL in canonical form (`{scheme}://{host}{path}[?{query}]`).
    Url {
        scheme: String,
        host: String,
        path: String,
        query: Option<String>,
    },
    /// A file: in a repository (`host` = `<host>/<owner>/<name>`, or a
    /// local repository's path), or on the agent's own machine (`None`).
    File { host: Option<String>, path: String },
    /// A forge repository.
    Repository(Repository),
    /// A key that is no URL: a path the shell could not resolve (keyed by
    /// the tool), or a URL whose host does not parse (`<url>`).
    Opaque { tool: String, key: String },
}

impl Loc {
    /// The URL text of a canonical URL, as `a2a-bench-resource` writes it.
    pub fn url(text: &str) -> Option<Self> {
        let (scheme, rest) = text.split_once("://")?;
        let host_end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (host, rest) = rest.split_at(host_end);
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (rest, None),
        };
        Some(Self::Url {
            scheme: scheme.to_owned(),
            host: host.to_owned(),
            path: if path.is_empty() {
                "/".to_owned()
            } else {
                path.to_owned()
            },
            query,
        })
    }

    /// `text` as written in a URL operand: the bench's normalized URL (an
    /// invalid host is `Opaque` on `<url>`); `None` when it is no URL.
    pub fn normalized(text: &str) -> Option<Self> {
        match normalized_url(text).ok()? {
            Resource::Url(url) => Self::url(&url),
            Resource::Opaque { tool, key } => Some(Self::Opaque { tool, key }),
            _ => None,
        }
    }

    /// A URL's text; `None` for another locator.
    pub fn url_text(&self) -> Option<String> {
        match self {
            Self::Url {
                scheme,
                host,
                path,
                query,
            } => Some(match query {
                Some(query) => format!("{scheme}://{host}{path}?{query}"),
                None => format!("{scheme}://{host}{path}"),
            }),
            Self::File { .. } | Self::Repository(_) | Self::Opaque { .. } => None,
        }
    }

    /// The kind of a shared resource; `None` for one only its agent's own
    /// computer holds (a local file or bare repository, a file in a local
    /// clone) or an opaque key.
    pub fn kind(&self) -> Option<ResourceKind> {
        match self {
            Self::Repository(_) => Some(ResourceKind::Repository),
            Self::File {
                host: Some(host), ..
            } if host.contains('/') && !host.starts_with('/') => Some(ResourceKind::RepoFile),
            Self::Url { .. } => Some(ResourceKind::Url),
            Self::File { .. } | Self::Opaque { .. } => None,
        }
    }

    /// The bench resource of a shared locator: a repository, a repository
    /// file, a forge thread or collection (a URL that is exactly such a
    /// page), else the URL. `None` when it is not shared.
    pub fn resource(&self) -> Option<Resource> {
        match self {
            Self::Repository(repo) => Some(Resource::Repository(repo.clone())),
            Self::File {
                host: Some(host),
                path,
            } if self.kind() == Some(ResourceKind::RepoFile) => {
                let (repo_host, rest) = host.split_once('/')?;
                let (owner, name) = rest.rsplit_once('/')?;
                Some(Resource::RepoFile {
                    repository: Repository {
                        host: repo_host.to_owned(),
                        owner: owner.to_owned(),
                        name: name.to_owned(),
                    },
                    path: path.clone(),
                })
            }
            Self::Url { .. } => {
                let text = self.url_text()?;
                let page = match canonical_url(&text) {
                    Ok(resource @ (Resource::Thread { .. } | Resource::Collection { .. }))
                        if Self::of_site(&resource).as_ref() == Some(self) =>
                    {
                        resource
                    }
                    _ => Resource::Url(text),
                };
                Some(page)
            }
            Self::File { .. } | Self::Opaque { .. } => None,
        }
    }

    /// The locator of a resource a site rule named: a thread or collection
    /// is its canonical web page (GitLab's style on `gitlab.com`, GitHub's
    /// elsewhere), a repository file the file of its repository.
    pub fn of_site(resource: &Resource) -> Option<Self> {
        match resource {
            Resource::Repository(repo) => Some(Self::Repository(repo.clone())),
            Resource::RepoFile { repository, path } => Some(Self::File {
                host: Some(repo_id(repository)),
                path: path.clone(),
            }),
            Resource::Thread {
                repository,
                kind,
                number,
            } => {
                let style = ForgeStyle::of_host(&repository.host);
                let kind = match kind {
                    ThreadKind::MergeRequest => Thread::Change,
                    // GitHub pulls are issues (one number space), as
                    // crosstalk's locator reads them.
                    ThreadKind::Issue => Thread::Issue,
                };
                Some(ForgeRepo(repository).thread(style, kind, *number))
            }
            Resource::Collection { repository, kind } => {
                let style = ForgeStyle::of_host(&repository.host);
                let kind = match kind {
                    CollectionKind::Issues => Thread::Issue,
                    CollectionKind::Pulls | CollectionKind::MergeRequests => Thread::Change,
                };
                Some(ForgeRepo(repository).collection(style, kind))
            }
            Resource::Url(text) => Self::url(text),
            Resource::Opaque { tool, key } => Some(Self::Opaque {
                tool: tool.clone(),
                key: key.clone(),
            }),
            Resource::File { host, path } => Some(Self::File {
                host: host.clone(),
                path: path.clone(),
            }),
            // No shell command names an MCP tool's resource, and it is not
            // a shared web resource (crosstalk's `Locator::Mcp` has no kind).
            Resource::Mcp { .. } => None,
        }
    }
}

impl fmt::Display for Loc {
    /// `repo://h/o/n`, `file://<host><path>`, the URL, `opaque://<tool>/<key>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url { .. } => f.write_str(&self.url_text().unwrap_or_default()),
            Self::File {
                host: Some(host),
                path,
            } => write!(f, "file://{host}{path}"),
            Self::File { host: None, path } => write!(f, "file://{path}"),
            Self::Repository(repo) => {
                write!(f, "repo://{}/{}/{}", repo.host, repo.owner, repo.name)
            }
            Self::Opaque { tool, key } => write!(f, "opaque://{tool}/{key}"),
        }
    }
}

/// `<host>/<owner>/<name>`: a repository's id, the host of its files.
pub fn repo_id(repo: &Repository) -> String {
    format!("{}/{}/{}", repo.host, repo.owner, repo.name)
}

/// How a forge spells its issue and change-request pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeStyle {
    GitHub,
    GitLab,
}

impl ForgeStyle {
    fn of_host(host: &str) -> Self {
        if host == "gitlab.com" {
            Self::GitLab
        } else {
            Self::GitHub
        }
    }
}

/// An issue, or a pull (GitHub) or merge (GitLab) request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thread {
    Issue,
    Change,
}

/// A forge repository's pages.
#[derive(Debug, Clone, Copy)]
pub struct ForgeRepo<'a>(pub &'a Repository);

impl ForgeRepo<'_> {
    /// Issue or change request `number`: GitHub `/<o>/<n>/issues/<N>` for
    /// both (one number space and one conversation), GitLab
    /// `/<o>/<n>/-/issues/<N>` and `/-/merge_requests/<N>`.
    pub fn thread(&self, style: ForgeStyle, kind: Thread, number: u64) -> Loc {
        let page = match (style, kind) {
            (ForgeStyle::GitHub, _) => format!("issues/{number}"),
            (ForgeStyle::GitLab, Thread::Issue) => format!("-/issues/{number}"),
            (ForgeStyle::GitLab, Thread::Change) => format!("-/merge_requests/{number}"),
        };
        self.page(&page)
    }

    /// The issues or change requests as a collection (a `create`, a
    /// `list`): GitHub `/issues`, `/pulls`; GitLab `/-/issues`,
    /// `/-/merge_requests`.
    pub fn collection(&self, style: ForgeStyle, kind: Thread) -> Loc {
        let page = match (style, kind) {
            (ForgeStyle::GitHub, Thread::Issue) => "issues",
            (ForgeStyle::GitHub, Thread::Change) => "pulls",
            (ForgeStyle::GitLab, Thread::Issue) => "-/issues",
            (ForgeStyle::GitLab, Thread::Change) => "-/merge_requests",
        };
        self.page(page)
    }

    fn page(&self, page: &str) -> Loc {
        Loc::Url {
            scheme: "https".to_owned(),
            host: self.0.host.clone(),
            path: format!("/{}/{}/{page}", self.0.owner, self.0.name),
            query: None,
        }
    }
}

/// An absolute POSIX path with `.`, `..`, repeated and trailing `/`
/// resolved lexically (`..` above the root stays at the root). Only
/// [`AbsolutePath::parse`] and [`AbsolutePath::join`] make one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AbsolutePath(String);

/// Why text is not a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("the path is empty")]
    Empty,
    #[error("the path contains a NUL byte")]
    Nul,
    #[error("the path is not absolute")]
    NotAbsolute,
}

fn check(text: &str) -> Result<(), PathError> {
    if text.is_empty() {
        return Err(PathError::Empty);
    }
    if text.contains('\0') {
        return Err(PathError::Nul);
    }
    Ok(())
}

impl AbsolutePath {
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// `text`, which must start with `/`, resolved.
    pub fn parse(text: &str) -> Result<Self, PathError> {
        check(text)?;
        if !text.starts_with('/') {
            return Err(PathError::NotAbsolute);
        }
        Ok(Self::root().join_checked(text))
    }

    /// `relative` resolved against this directory; an absolute one
    /// replaces it.
    pub fn join(&self, relative: &str) -> Result<Self, PathError> {
        check(relative)?;
        Ok(self.join_checked(relative))
    }

    fn join_checked(&self, relative: &str) -> Self {
        let mut segments: Vec<&str> = if relative.starts_with('/') {
            Vec::new()
        } else {
            self.0.split('/').filter(|s| !s.is_empty()).collect()
        };
        for segment in relative.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                name => segments.push(name),
            }
        }
        let mut path = String::with_capacity(relative.len() + self.0.len());
        for segment in &segments {
            path.push('/');
            path.push_str(segment);
        }
        if path.is_empty() {
            path.push('/');
        }
        Self(path)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A repository a clone is of: a forge's, or one on the local filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RepoId {
    Forge(Repository),
    /// A local repository's directory (no `.git`, no trailing `/`).
    Local(String),
}

impl RepoId {
    /// The repository a git remote names: `http(s)`, `ssh`, `git`,
    /// `git+ssh` or `ssh+git` `://[user@]host[:port]/path`, scp form
    /// `[user@]host:path` (a host with a dot, or `localhost`),
    /// `file:///path`, an absolute path, or a `.` path from `cwd`. The
    /// forms a forge remote takes are `a2a_bench_resource::canonical_remote`'s,
    /// and its parts are `a2a_bench_resource::repository`'s.
    pub fn parse(remote: &str, cwd: Option<&AbsolutePath>) -> Option<Self> {
        let remote = remote.trim();
        if remote.is_empty() || remote.contains(char::is_whitespace) {
            return None;
        }
        if let Some(path) = remote.strip_prefix("file://") {
            return Self::local(AbsolutePath::parse(path).ok()?);
        }
        if remote.contains("://") {
            return Self::forge_remote(remote);
        }
        if remote.starts_with('/') {
            return Self::local(AbsolutePath::parse(remote).ok()?);
        }
        if remote
            .split_once(':')
            .is_some_and(|(authority, path)| !authority.contains('/') && !path.starts_with("//"))
        {
            return Self::forge_remote(remote);
        }
        if remote.starts_with('.') {
            return Self::local(cwd?.join(remote).ok()?);
        }
        None
    }

    /// The forge repository a remote names, through
    /// `a2a_bench_resource::canonical_remote`; `None` for a local one.
    pub fn forge_remote(remote: &str) -> Option<Self> {
        canonical_remote(remote).ok().map(Self::Forge)
    }

    /// The repository at `path` (`owner/name`, `group/subgroup/name`) on
    /// `host`; `path` may carry `.git` and slashes around it.
    pub fn forge(host: &str, path: &str) -> Option<Self> {
        let path = path.trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, name) = path.rsplit_once('/')?;
        repository(host, owner, name).ok().map(Self::Forge)
    }

    fn local(path: AbsolutePath) -> Option<Self> {
        let text = path.as_str();
        let text = text
            .strip_suffix(".git")
            .unwrap_or(text)
            .trim_end_matches('/');
        (!text.is_empty()).then(|| Self::Local(text.to_owned()))
    }

    /// The repository's own locator: the forge repository, or the local
    /// directory's file.
    pub fn locator(&self) -> Loc {
        match self {
            Self::Forge(repo) => Loc::Repository(repo.clone()),
            Self::Local(path) => Loc::File {
                host: None,
                path: path.clone(),
            },
        }
    }

    pub fn forge_parts(&self) -> Option<&Repository> {
        match self {
            Self::Forge(repo) => Some(repo),
            Self::Local(_) => None,
        }
    }

    /// The file at `path` (absolute within the repository).
    pub fn file(&self, path: &AbsolutePath) -> Loc {
        let host = match self {
            Self::Forge(repo) => repo_id(repo),
            Self::Local(dir) => dir.clone(),
        };
        Loc::File {
            host: Some(host),
            path: path.as_str().to_owned(),
        }
    }
}

/// Which directories are clones of which repositories. A later binding of
/// the same directory replaces the earlier one; a path is in the clone
/// whose root is its longest ancestor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoBindings(Vec<(AbsolutePath, RepoId)>);

impl RepoBindings {
    pub fn bind(&mut self, root: AbsolutePath, repo: RepoId) {
        self.0.retain(|(bound, _)| *bound != root);
        self.0.push((root, repo));
    }

    /// The repository `path` is in and its path inside it.
    pub fn locate(&self, path: &AbsolutePath) -> Option<(&RepoId, AbsolutePath)> {
        self.0
            .iter()
            .filter_map(|(root, repo)| Some((root, repo, within(path, root)?)))
            .max_by_key(|(root, _, _)| root.as_str().len())
            .map(|(_, repo, inside)| (repo, inside))
    }
}

/// `path` relative to `root`, as an absolute path inside it.
fn within(path: &AbsolutePath, root: &AbsolutePath) -> Option<AbsolutePath> {
    if root.as_str() == "/" {
        return Some(path.clone());
    }
    let rest = path.as_str().strip_prefix(root.as_str())?;
    if rest.is_empty() {
        return Some(AbsolutePath::root());
    }
    rest.starts_with('/')
        .then(|| AbsolutePath::parse(rest).ok())
        .flatten()
}

/// What a file path resolves against: the working directory and the
/// clones.
#[derive(Debug, Clone, Copy)]
pub struct FileScope<'a> {
    pub cwd: Option<&'a AbsolutePath>,
    pub repos: &'a RepoBindings,
}

/// The tool a path the shell cannot resolve is keyed by.
pub const SHELL_TOOL: &str = "bash";

/// The locator of the file `written` names: an absolute path (or a
/// relative one resolved against the working directory) in a clone is the
/// repository's file, elsewhere the agent's own file; a relative path with
/// no working directory, a `~` path or a Windows path is `Opaque`.
pub fn file_locator(written: &str, scope: FileScope<'_>) -> Result<Loc, PathError> {
    check(written)?;
    let absolute = if written.starts_with('/') {
        AbsolutePath::parse(written)?
    } else if written.starts_with('~') || is_windows(written) {
        return Ok(opaque(written));
    } else {
        match scope.cwd {
            Some(cwd) => cwd.join(written)?,
            None => return Ok(opaque(written)),
        }
    };
    Ok(absolute_locator(absolute, scope))
}

/// The locator of an absolute path in `scope`.
pub fn absolute_locator(path: AbsolutePath, scope: FileScope<'_>) -> Loc {
    if let Some((repo, inside)) = scope.repos.locate(&path) {
        return repo.file(&inside);
    }
    Loc::File {
        host: None,
        path: path.0,
    }
}

fn opaque(key: &str) -> Loc {
    Loc::Opaque {
        tool: SHELL_TOOL.to_owned(),
        key: key.to_owned(),
    }
}

/// `C:\x`, `C:/x`, `\\server\share`.
fn is_windows(text: &str) -> bool {
    let bytes = text.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive || text.starts_with("\\\\")
}
