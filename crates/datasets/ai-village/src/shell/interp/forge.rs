//! The forge CLIs, `gh` (GitHub) and `glab` (GitLab): clones, issues and
//! pull/merge requests, and their API commands.
//!
//! - `gh repo clone <repo> [<dir>]`, `glab repo clone <repo> [<dir>]`:
//!   clone (bind the directory, read the repository) as `git clone` does.
//! - `gh issue|pr`, `glab issue|mr` `<verb>`:
//!
//!   | Verb | Access | Locator |
//!   | --- | --- | --- |
//!   | `create` | write | the collection: the number is not known from the call |
//!   | `comment`, `note`, `edit`, `update`, `review` | write | the thread of the number or URL operand, else the collection |
//!   | `view` | read | the thread, else the collection |
//!   | `list` | read | the collection |
//!
//!   The repository is `-R`/`--repo` (`owner/name`, `host/owner/name`,
//!   `group/subgroup/name` or a URL), else the one the clone the command
//!   runs in is bound to; without one there is no access. A URL operand
//!   names its thread through the CLI's forge's site rule.
//! - `gh api <endpoint>`, `glab api <endpoint>`: an HTTP request to
//!   `https://api.github.com/<endpoint>` (`https://<hostname>/api/v3/…`
//!   with `--hostname`) or `https://gitlab.com/api/v4/<endpoint>`,
//!   `{owner}`/`{repo}` (gh) and `:fullpath`/`:id`/`:namespace`/`:group`/
//!   `:repo` (glab) filled from the bound repository. The method is
//!   `-X`/`--method`, else `POST` with a field (`-f`, `-F`, `--raw-field`,
//!   `--field`) or `--input`, else `GET`. `graphql` is no access.
//!
//! Every access is judged by the CLI's output (`CommandRule::ForgeCli`;
//! clones by `CommandRule::GitTransfer`).

use a2a_bench_format::resource::Resource;
use a2a_bench_resource::canonical_url;

use super::super::Candidate;
use super::super::http::{self, HttpRequest, Method, form_fields};
use super::super::lex::Word;
use super::super::locator::{ForgeRepo, ForgeStyle, Loc, RepoId, Thread};
use super::super::options::{OptSpec, Options};
use super::super::outcome::CommandRule;
use super::Interpreter;

/// Which forge CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cli {
    Gh,
    Glab,
}

impl Cli {
    fn style(self) -> ForgeStyle {
        match self {
            Self::Gh => ForgeStyle::GitHub,
            Self::Glab => ForgeStyle::GitLab,
        }
    }

    fn default_host(self) -> &'static str {
        match self {
            Self::Gh => "github.com",
            Self::Glab => "gitlab.com",
        }
    }

    fn threads(self) -> &'static OptSpec {
        match self {
            Self::Gh => &GH_THREADS,
            Self::Glab => &GLAB_THREADS,
        }
    }

    /// Whether `host` is one this CLI's forge serves its pages on.
    fn serves(self, host: &str) -> bool {
        match self {
            Self::Gh => {
                matches!(
                    host,
                    "github.com"
                        | "www.github.com"
                        | "codeload.github.com"
                        | "raw.githubusercontent.com"
                        | "api.github.com"
                ) || host.ends_with(".github.io")
            }
            Self::Glab => {
                matches!(host, "gitlab.com" | "www.gitlab.com") || host.ends_with(".gitlab.io")
            }
        }
    }
}

const CLONE: OptSpec = OptSpec {
    short_values: "u",
    long_values: &["--upstream-remote-name"],
};

const THREAD_LONG_VALUES: &[&str] = &[
    "--repo",
    "--title",
    "--body",
    "--body-file",
    "--description",
    "--message",
    "--assignee",
    "--label",
    "--milestone",
    "--project",
    "--base",
    "--head",
    "--reviewer",
    "--limit",
    "--state",
    "--search",
    "--jq",
    "--template",
    "--json",
    "--author",
    "--app",
    "--mention",
    "--add-label",
    "--remove-label",
    "--add-assignee",
    "--remove-assignee",
    "--add-reviewer",
    "--remove-reviewer",
    "--add-project",
    "--remove-project",
    "--target-branch",
    "--source-branch",
    "--page",
    "--per-page",
    "--output",
];

const GH_THREADS: OptSpec = OptSpec {
    short_values: "RtbFalmpBHrLsSqA",
    long_values: THREAD_LONG_VALUES,
};

const GLAB_THREADS: OptSpec = OptSpec {
    short_values: "RtdmlasbpPF",
    long_values: THREAD_LONG_VALUES,
};

const API: OptSpec = OptSpec {
    short_values: "XfFHqtp",
    long_values: &[
        "--method",
        "--raw-field",
        "--field",
        "--header",
        "--input",
        "--jq",
        "--template",
        "--hostname",
        "--cache",
        "--preview",
    ],
};

impl Interpreter {
    pub(super) fn forge_cli(
        &mut self,
        cli: Cli,
        args: &[Word],
        stdout_to_file: bool,
        found: &mut Vec<Candidate>,
    ) {
        let Some((group, rest)) = args.split_first() else {
            return;
        };
        match (cli, group.text.as_str()) {
            (_, "repo") => self.forge_clone(cli, rest, found),
            (_, "issue") => self.thread_command(cli, Thread::Issue, rest, found),
            (Cli::Gh, "pr") | (Cli::Glab, "mr") => {
                self.thread_command(cli, Thread::Change, rest, found);
            }
            (_, "api") => self.api(cli, rest, stdout_to_file, found),
            _ => {}
        }
    }

    fn forge_clone(&mut self, cli: Cli, args: &[Word], found: &mut Vec<Candidate>) {
        let Some((verb, rest)) = args.split_first() else {
            return;
        };
        if verb.text != "clone" {
            return;
        }
        let parsed = Options::parse(rest, &CLONE);
        let dir = self.state.cwd.clone();
        if let Some(repo) = self.clone_into(dir.as_ref(), &parsed.operands, |remote, cwd| {
            repo_argument(cli, remote).or_else(|| RepoId::parse(remote, cwd))
        }) {
            found.push(Candidate::read(repo.locator()).judged_by(CommandRule::GitTransfer));
        }
    }

    fn thread_command(&self, cli: Cli, kind: Thread, args: &[Word], found: &mut Vec<Candidate>) {
        let Some((verb, rest)) = args.split_first() else {
            return;
        };
        let parsed = Options::parse(rest, cli.threads());
        let target = parsed.operands.first().and_then(|word| word.as_literal());
        let url_thread = target
            .filter(|text| text.contains("://"))
            .and_then(|text| thread_url(cli, text));
        let number = target.and_then(thread_number);
        let repo = match parsed.last(&["-R", "--repo"]) {
            Some(word) => word.as_literal().and_then(|text| repo_argument(cli, text)),
            None => self.bound_repo(self.state.cwd.as_ref()),
        };
        let thread = || -> Option<Loc> {
            if let Some(url) = &url_thread {
                return Some(url.clone());
            }
            let forge = ForgeRepo(repo.as_ref()?.forge_parts()?);
            Some(match number {
                Some(number) => forge.thread(cli.style(), kind, number),
                None => forge.collection(cli.style(), kind),
            })
        };
        let collection = || -> Option<Loc> {
            let forge = ForgeRepo(repo.as_ref()?.forge_parts()?);
            Some(forge.collection(cli.style(), kind))
        };
        let candidate = match verb.text.as_str() {
            "create" => collection().map(Candidate::write),
            "comment" | "note" | "edit" | "update" | "review" => thread().map(Candidate::write),
            "view" => thread().map(Candidate::read),
            "list" => collection().map(Candidate::read),
            _ => None,
        };
        if let Some(candidate) = candidate {
            found.push(candidate.judged_by(CommandRule::ForgeCli));
        }
    }

    fn api(&self, cli: Cli, args: &[Word], stdout_to_file: bool, found: &mut Vec<Candidate>) {
        let parsed = Options::parse(args, &API);
        let Some(endpoint) = parsed.operands.first().and_then(|word| word.as_literal()) else {
            return;
        };
        if endpoint == "graphql" {
            return;
        }
        let Some(endpoint) = self.fill_placeholders(cli, endpoint) else {
            return;
        };
        let url = if endpoint.contains("://") {
            endpoint
        } else {
            let endpoint = endpoint.trim_start_matches('/');
            match (cli, parsed.last(&["--hostname"]).and_then(Word::as_literal)) {
                (Cli::Gh, Some(host)) if host != "github.com" => {
                    format!("https://{host}/api/v3/{endpoint}")
                }
                (Cli::Gh, _) => format!("https://api.github.com/{endpoint}"),
                (Cli::Glab, _) => format!("https://gitlab.com/api/v4/{endpoint}"),
            }
        };
        let Some(url) = Loc::normalized(&url) else {
            return;
        };
        let fields: Vec<(String, String)> = parsed
            .with_values()
            .filter(|(name, _)| matches!(*name, "-f" | "-F" | "--raw-field" | "--field"))
            .filter_map(|(_, value)| value.as_literal())
            .flat_map(form_fields)
            .collect();
        let method = match parsed.last(&["-X", "--method"]) {
            Some(method) => Method::parse(&method.text),
            None if !fields.is_empty() || parsed.has(&["--input"]) => Method::Post,
            None => Method::Get,
        };
        let request = HttpRequest {
            url,
            method,
            form: fields,
        };
        http::candidates(&request, !stdout_to_file, CommandRule::ForgeCli, found);
    }

    /// `endpoint` with its repository placeholders filled from the bound
    /// repository; `None` when one is left that cannot be filled.
    fn fill_placeholders(&self, cli: Cli, endpoint: &str) -> Option<String> {
        let placeholders: &[&str] = match cli {
            Cli::Gh => &["{owner}", "{repo}"],
            Cli::Glab => &[":fullpath", ":id", ":namespace", ":group", ":repo"],
        };
        if !placeholders.iter().any(|p| endpoint.contains(p)) {
            return Some(endpoint.to_owned());
        }
        let repo = self.bound_repo(self.state.cwd.as_ref())?;
        let forge = repo.forge_parts()?;
        let encoded = format!("{}%2F{}", forge.owner.replace('/', "%2F"), forge.name);
        let filled = match cli {
            Cli::Gh => endpoint
                .replace("{owner}", &forge.owner)
                .replace("{repo}", &forge.name),
            Cli::Glab => endpoint
                .replace(":fullpath", &encoded)
                .replace(":id", &encoded)
                .replace(":namespace", &forge.owner)
                .replace(":group", &forge.owner)
                .replace(":repo", &forge.name),
        };
        Some(filled)
    }
}

/// A repository argument (`-R`, `repo clone`): `owner/name`,
/// `host/owner/name` (a first segment with a dot is a host),
/// `group/subgroup/name`, or a URL or git remote.
fn repo_argument(cli: Cli, text: &str) -> Option<RepoId> {
    if text.contains("://") || text.contains('@') {
        return RepoId::parse(text, None);
    }
    match text.split_once('/') {
        Some((host, path)) if host.contains('.') && path.contains('/') => RepoId::forge(host, path),
        _ => RepoId::forge(cli.default_host(), text),
    }
}

/// `123` or `#123`.
fn thread_number(text: &str) -> Option<u64> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The thread or collection a forge URL operand names
/// (`https://github.com/o/n/pull/5`), read through the CLI's forge's site
/// rule; `None` for any other page.
fn thread_url(cli: Cli, text: &str) -> Option<Loc> {
    let url = Loc::normalized(text)?;
    let Loc::Url { host, .. } = &url else {
        return None;
    };
    if !cli.serves(host) {
        return None;
    }
    match canonical_url(&url.url_text()?).ok()? {
        resource @ (Resource::Thread { .. } | Resource::Collection { .. }) => {
            Loc::of_site(&resource)
        }
        _ => None,
    }
}
