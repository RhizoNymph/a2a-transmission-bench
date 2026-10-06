//! One agent's shell between commands: its working directory and the
//! clones it knows, and how a command and its output move them.
//!
//! [`Context::observe`] is what one executed command teaches, given the
//! interpreter's run of it from this context and its output:
//!
//! 1. the clones the command made or named (`git clone`, `gh`/`glab repo
//!    clone`, `git remote add|set-url`) replace the known ones;
//! 2. when the command ran exactly one remote query (`git remote -v`,
//!    `get-url`, `git config remote.<name>.url`) in a known directory,
//!    that directory is bound to the first remote the output shows (a
//!    word with `://`, a `:` or a leading `/` that is a remote, from the
//!    directory the command started in);
//! 3. the working directory becomes the one the last command left (the
//!    village's bash tool is one persistent shell per agent), unknown when
//!    a `cd` could not be followed;
//! 4. a line `Shell cwd was reset to <path>` sets it to `<path>`.
//!
//! A command the lexer refuses teaches nothing.

use super::interp::{ShellRun, ShellState};
use super::locator::{AbsolutePath, RepoBindings, RepoId};

/// A shell's working directory and its known clones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    cwd: Option<AbsolutePath>,
    repos: RepoBindings,
}

/// A harness's note that it moved the shell back.
const CWD_RESET: &str = "Shell cwd was reset to ";

impl Context {
    pub fn new(cwd: Option<AbsolutePath>) -> Self {
        Self {
            cwd,
            repos: RepoBindings::default(),
        }
    }

    pub fn cwd(&self) -> Option<&AbsolutePath> {
        self.cwd.as_ref()
    }

    pub fn repos(&self) -> &RepoBindings {
        &self.repos
    }

    /// Records that `root` is a clone of `repo`.
    pub fn bind_repo(&mut self, root: AbsolutePath, repo: RepoId) {
        self.repos.bind(root, repo);
    }

    /// Where a command starts.
    pub fn start(&self) -> ShellState {
        ShellState {
            cwd: self.cwd.clone(),
            repos: self.repos.clone(),
        }
    }

    /// Learns from one command's run (from this context) and its output.
    pub fn observe(&mut self, run: ShellRun, output: &str) {
        self.repos = run.end.repos;
        if let [Some(dir)] = run.remote_queries.as_slice()
            && let Some(repo) = printed_remote(output, self.cwd.as_ref())
        {
            self.repos.bind(dir.clone(), repo);
        }
        self.cwd = run.end.cwd;
        if let Some(reset) = output
            .lines()
            .find_map(|line| line.trim().strip_prefix(CWD_RESET))
            .and_then(|path| AbsolutePath::parse(path.trim()).ok())
        {
            self.cwd = Some(reset);
        }
    }
}

/// The first remote a `git remote -v` (or `get-url`) output shows.
fn printed_remote(text: &str, cwd: Option<&AbsolutePath>) -> Option<RepoId> {
    text.lines().find_map(|line| {
        line.split_whitespace()
            .filter(|word| word.contains("://") || word.contains(':') || word.starts_with('/'))
            .find_map(|word| RepoId::parse(word, cwd))
    })
}
