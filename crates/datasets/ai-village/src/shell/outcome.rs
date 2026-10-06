//! A write's outcome, from the output of the command that made it.
//!
//! The village records no exit status: every bash result is a success on
//! the wire, so a command's own output is all that says whether it
//! failed. An access a known command makes is judged by that command's
//! rule over the whole call's output (a script's commands share one
//! result, so a rule reads every line of it):
//!
//! | Command | `Rejected` | `Delivered` | otherwise |
//! | --- | --- | --- | --- |
//! | `git push` | a line opening with `! [rejected]`, `! [remote rejected]`, `error:`, `fatal:`, `remote: Permission`, `remote: Invalid`, `Permission denied` | a ref update (`a..b  main -> main`, `* [new branch]`), `Everything up-to-date` | `Unknown` |
//! | `git pull`, `fetch`, `clone`, `gh`/`glab repo clone` | a line opening with `fatal:` or `error:` | | `Delivered` |
//! | `curl`, `wget` | a `curl: (N)` line, or the last HTTP status shown is 4xx/5xx (a `HTTP/x 404` status line, wget's `… 404 Not Found` and `ERROR 404:`, the code a `-w '%{http_code}'` printed last) | the last status shown is 2xx/3xx | `Delivered` |
//! | `gh`, `glab` | a line opening with `gh: `, `glab: `, `GraphQL:`, `error:`, `ERROR:`, `HTTP 4`/`HTTP 5`, `could not`, `failed to`, `X `, or ending in `(HTTP 4xx)`/`(HTTP 5xx)` | a printed `https://` URL or a `✓` line | `Delivered` |
//! | anything else (redirections, `tee`, `cat`, `sed -n`, `curl -o`) | | | `Delivered` |
//!
//! A read is kept only when its command's rule does not say `Rejected`.

/// Whether a write reached its resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WriteOutcome {
    Delivered,
    Rejected,
    Unknown,
}

impl WriteOutcome {
    /// Whether a write with this outcome can pair with a read: every
    /// outcome but `Rejected`.
    pub fn pairs(self) -> bool {
        match self {
            Self::Delivered | Self::Unknown => true,
            Self::Rejected => false,
        }
    }
}

/// The command an access came from, whose output judges it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandRule {
    /// `git push`.
    GitPush,
    /// `git pull`, `git fetch`, `git clone`, `gh`/`glab repo clone`.
    GitTransfer,
    /// `curl`, `wget`. `status_written`: a `-w` format prints the status
    /// code (`%{http_code}`, `%{response_code}`).
    Http { status_written: bool },
    /// `gh`, `glab`.
    ForgeCli,
}

impl CommandRule {
    /// What `text`, the command's output, says: `None` when it says
    /// neither.
    pub fn judge(self, text: &str) -> Option<WriteOutcome> {
        let lines = || text.lines().map(str::trim).filter(|line| !line.is_empty());
        let opens = |markers: &[&str]| {
            lines().any(|line| markers.iter().any(|marker| line.starts_with(marker)))
        };
        match self {
            Self::GitPush => {
                if opens(&GIT_PUSH_FAILURES) {
                    Some(WriteOutcome::Rejected)
                } else if lines().any(ref_updated) {
                    Some(WriteOutcome::Delivered)
                } else {
                    Some(WriteOutcome::Unknown)
                }
            }
            Self::GitTransfer => opens(&["fatal:", "error:"]).then_some(WriteOutcome::Rejected),
            Self::Http { status_written } => {
                if opens(&["curl: ("]) {
                    return Some(WriteOutcome::Rejected);
                }
                let mut status = lines().rev().find_map(shown_status);
                if status_written && let Some(written) = lines().next_back().and_then(trailing_code)
                {
                    status = Some(written);
                }
                status.map(|code| {
                    if code >= 400 {
                        WriteOutcome::Rejected
                    } else {
                        WriteOutcome::Delivered
                    }
                })
            }
            Self::ForgeCli => {
                let failed = opens(&FORGE_CLI_FAILURES)
                    || lines().any(|line| {
                        line.ends_with(')')
                            && line
                                .rsplit_once("(HTTP ")
                                .and_then(|(_, code)| code.strip_suffix(')'))
                                .and_then(|code| code.parse::<u16>().ok())
                                .is_some_and(|code| code >= 400)
                    });
                if failed {
                    Some(WriteOutcome::Rejected)
                } else if lines().any(|line| line.starts_with("https://") || line.starts_with('✓'))
                {
                    Some(WriteOutcome::Delivered)
                } else {
                    None
                }
            }
        }
    }
}

/// The outcome of an access judged by `rule` (`None`: no command rule)
/// over the call's output.
pub fn judge(rule: Option<CommandRule>, output: &str) -> WriteOutcome {
    rule.and_then(|rule| rule.judge(output))
        .unwrap_or(WriteOutcome::Delivered)
}

const GIT_PUSH_FAILURES: [&str; 7] = [
    "! [rejected]",
    "! [remote rejected]",
    "error:",
    "fatal:",
    "remote: Permission",
    "remote: Invalid",
    "Permission denied",
];

const FORGE_CLI_FAILURES: [&str; 10] = [
    "gh: ",
    "glab: ",
    "GraphQL:",
    "error:",
    "ERROR:",
    "HTTP 4",
    "HTTP 5",
    "could not",
    "failed to",
    "X ",
];

/// A git push ref-update line: `a1b2c3d..e4f5a6b  main -> main`, a forced
/// `+ a...b main -> main (forced update)`, `* [new branch]  x -> x`.
fn ref_updated(line: &str) -> bool {
    if line == "Everything up-to-date" {
        return true;
    }
    if !line.contains(" -> ") || line.starts_with('!') {
        return false;
    }
    let first = line.trim_start_matches(['+', ' ']);
    first.starts_with("* [new ")
        || first
            .split_whitespace()
            .next()
            .and_then(|range| range.split_once(".."))
            .is_some_and(|(from, to)| {
                let to = to.trim_start_matches('.');
                hex(from) && hex(to)
            })
}

fn hex(text: &str) -> bool {
    text.len() >= 4 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The status a line shows: `HTTP/1.1 404 Not Found`, `HTTP/2 200`, a
/// `< HTTP/2 200` from `-v`, wget's `HTTP request sent, awaiting
/// response... 404 Not Found` and `ERROR 404: Not Found.`.
fn shown_status(line: &str) -> Option<u16> {
    let line = line.trim_start_matches(['<', ' ']);
    let code = if let Some(rest) = line.strip_prefix("HTTP/") {
        rest.split_whitespace().nth(1)?
    } else if let Some(rest) = line.strip_prefix("ERROR ") {
        rest.split(':').next()?
    } else if let Some((_, rest)) = line.split_once("awaiting response... ") {
        rest.split_whitespace().next()?
    } else {
        return None;
    };
    status_code(code)
}

/// A three-digit status code at the end of a line (`404`, `HTTP 404`,
/// `{"ok":true}200`).
fn trailing_code(line: &str) -> Option<u16> {
    let digits = line.len() - line.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits != 3 {
        return None;
    }
    status_code(line.get(line.len() - 3..)?)
}

fn status_code(text: &str) -> Option<u16> {
    let code: u16 = text.parse().ok()?;
    (text.len() == 3 && (100..600).contains(&code)).then_some(code)
}
