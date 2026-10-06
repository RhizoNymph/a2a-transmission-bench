//! The bench's shell model: which shared resources a village bash command
//! read or wrote, decided from the command and its recorded output alone.
//!
//! This is the bench's own definition (its normative table is in
//! `docs/features/dataset-ai-village.md`); no detector's extractor is
//! consulted. Version `ai-village@1` reproduces the accesses crosstalk's
//! L5 extractor (crosstalk-flow at 7f8a2fb) gave ct-eval's converter, so
//! its quirks are kept on purpose and listed in the doc.
//!
//! ```text
//! Shell::accesses(command, output)
//!   expand_home(command)                       ~ and $HOME are /home/computeruse
//!   lex::Script::lex                           refused: no access, counted
//!   interp::run(script, context.start())       candidates (op, locator, rule), end state
//!   classify(candidates, output)               outcome::judge; reads judged Rejected dropped; duplicates dropped
//!   context.observe(run, output)               clones, remote -v, cwd, "Shell cwd was reset"
//!   printed remote ("To <r>"/"From <r>")       binds the unbound end directory; the command is run again with it
//!   keep shared locators (Loc::kind)           payload: unseen (git push) or payload::authored(command)
//! ```

pub mod context;
pub mod http;
pub mod interp;
pub mod lex;
pub mod locator;
pub mod options;
pub mod outcome;
pub mod payload;
pub mod split;

use a2a_bench_format::resource::Resource;
use a2a_bench_resource::ResourceKind;

use context::Context;
use lex::Script;
use locator::{AbsolutePath, Loc, RepoId};
use outcome::{CommandRule, WriteOutcome};

/// The bash tool's home directory in the village's computers.
pub const HOME: &str = "/home/computeruse";

/// Whether an access reads or writes, before its outcome is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Read,
    Write,
}

/// An access a command names, before its output is judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: Kind,
    pub locator: Loc,
    /// The write's content is not in the command (`git push`).
    pub unseen: bool,
    /// The command whose output judges it; `None`: delivered.
    pub rule: Option<CommandRule>,
}

impl Candidate {
    pub fn read(locator: Loc) -> Self {
        Self {
            kind: Kind::Read,
            locator,
            unseen: false,
            rule: None,
        }
    }

    pub fn write(locator: Loc) -> Self {
        Self {
            kind: Kind::Write,
            locator,
            unseen: false,
            rule: None,
        }
    }

    /// This write's content is not in the command.
    pub fn unseen(mut self) -> Self {
        self.unseen = true;
        self
    }

    pub fn judged_by(mut self, rule: CommandRule) -> Self {
        self.rule = Some(rule);
        self
    }
}

/// What a write carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// The content is not in the command (`git push`): only co-access can
    /// link the write.
    Unseen,
    /// The texts the author typed (here-document bodies, body flags,
    /// data); may be empty.
    Authored(Vec<String>),
}

/// A read, or a write with its outcome and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Read,
    Write {
        outcome: WriteOutcome,
        payload: Payload,
    },
}

impl Op {
    pub fn is_write(&self) -> bool {
        matches!(self, Self::Write { .. })
    }

    /// Whether this access can be one side of a pair: every read, and
    /// every write not rejected.
    pub fn pairs(&self) -> bool {
        match self {
            Self::Read => true,
            Self::Write { outcome, .. } => outcome.pairs(),
        }
    }
}

/// One read or write of a shared resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub op: Op,
    pub locator: Loc,
    pub kind: ResourceKind,
}

impl Access {
    /// What a write carried; `None` for a read.
    pub fn payload(&self) -> Option<&Payload> {
        match &self.op {
            Op::Read => None,
            Op::Write { payload, .. } => Some(payload),
        }
    }

    /// `<read|write> <kind>`, for counts.
    pub fn label(&self) -> String {
        let op = if self.op.is_write() { "write" } else { "read" };
        format!("{op} {}", self.kind.as_str())
    }

    /// The bench resource a label's channel names.
    pub fn resource(&self) -> Option<Resource> {
        self.locator.resource()
    }
}

/// An access judged by the output, before the shared-resource filter.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Judged {
    write: Option<WriteOutcome>,
    unseen: bool,
    locator: Loc,
}

/// One agent's persistent shell.
#[derive(Debug, Clone)]
pub struct Shell {
    context: Context,
    /// Commands the lexer refused (an unterminated quote or substitution,
    /// a redirection without a target): no access.
    unextracted: u64,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            context: Context::new(AbsolutePath::parse(HOME).ok()),
            unextracted: 0,
        }
    }
}

impl Shell {
    /// The shell's working directory, when it is known.
    pub fn cwd(&self) -> Option<&str> {
        self.context.cwd().map(AbsolutePath::as_str)
    }

    /// How many commands the lexer refused.
    pub fn unextracted(&self) -> u64 {
        self.unextracted
    }

    /// The shared-resource accesses of one executed command, given its
    /// output (stdout and stderr together). Moves the shell and learns its
    /// clones.
    pub fn accesses(&mut self, command: &str, output: &str) -> Vec<Access> {
        let expanded = expand_home(command);
        let script = match Script::lex(&expanded) {
            Ok(script) => Some(script),
            Err(error) => {
                tracing::debug!(?error, "bash command not lexed");
                self.unextracted += 1;
                None
            }
        };
        let mut after = self.context.clone();
        let mut judged = Vec::new();
        if let Some(script) = &script {
            let run = interp::run(script, self.context.start());
            judged = classify(&run.candidates, output);
            after.observe(run, output);
        }
        if let Some(dir) = after.cwd().cloned()
            && after.repos().locate(&dir).is_none()
            && let Some(repo) = printed_remote(&expanded, output)
        {
            let mut before = self.context.clone();
            before.bind_repo(dir.clone(), repo.clone());
            after.bind_repo(dir, repo);
            judged = match &script {
                Some(script) => classify(&interp::run(script, before.start()).candidates, output),
                None => Vec::new(),
            };
        }
        self.context = after;
        let authored = payload::authored(command);
        judged
            .into_iter()
            .filter_map(|access| {
                let kind = access.locator.kind()?;
                let op = match access.write {
                    None => Op::Read,
                    Some(outcome) if access.unseen => Op::Write {
                        outcome,
                        payload: Payload::Unseen,
                    },
                    Some(outcome) => Op::Write {
                        outcome,
                        payload: Payload::Authored(authored.clone()),
                    },
                };
                Some(Access {
                    op,
                    locator: access.locator,
                    kind,
                })
            })
            .collect()
    }
}

/// Each candidate judged by the output: a write with its outcome, a read
/// only when its command's rule does not reject it; an access already
/// found is not repeated.
fn classify(candidates: &[Candidate], output: &str) -> Vec<Judged> {
    let mut judged: Vec<Judged> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let outcome = outcome::judge(candidate.rule, output);
        let access = match candidate.kind {
            Kind::Write => Judged {
                write: Some(outcome),
                unseen: candidate.unseen,
                locator: candidate.locator.clone(),
            },
            Kind::Read if outcome != WriteOutcome::Rejected => Judged {
                write: None,
                unseen: false,
                locator: candidate.locator.clone(),
            },
            Kind::Read => continue,
        };
        if !judged.contains(&access) {
            judged.push(access);
        }
    }
    judged
}

/// The forge repository a `git push`, `pull` or `fetch` printed as its
/// remote (`To <remote>`, `From <remote>`).
fn printed_remote(command: &str, output: &str) -> Option<RepoId> {
    let git = command.contains("git");
    let markers: Vec<&str> = [
        (git && command.contains("push"), "To "),
        (
            git && (command.contains("pull") || command.contains("fetch")),
            "From ",
        ),
    ]
    .into_iter()
    .filter_map(|(ran, marker)| ran.then_some(marker))
    .collect();
    output.lines().find_map(|line| {
        let line = line.trim();
        let remote = markers
            .iter()
            .find_map(|marker| line.strip_prefix(marker))?
            .split_whitespace()
            .next()?;
        match RepoId::parse(remote, None)? {
            repo @ RepoId::Forge(_) => Some(repo),
            RepoId::Local(_) => None,
        }
    })
}

/// `command` with `~` (a word's leading `~` before `/` or the word's end)
/// and `$HOME` / `${HOME}` replaced by [`HOME`].
pub fn expand_home(command: &str) -> String {
    let command = command.replace("${HOME}", HOME).replace("$HOME", HOME);
    let mut out = String::with_capacity(command.len());
    let mut previous: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        let word_start = previous.is_none_or(|p| {
            p.is_whitespace() || matches!(p, '=' | ';' | '&' | '|' | '(' | '"' | '\'')
        });
        let ends = chars.peek().is_none_or(|next| {
            *next == '/'
                || next.is_whitespace()
                || matches!(next, ';' | '&' | '|' | ')' | '"' | '\'')
        });
        if c == '~' && word_start && ends {
            out.push_str(HOME);
        } else {
            out.push(c);
        }
        previous = Some(c);
    }
    out
}
