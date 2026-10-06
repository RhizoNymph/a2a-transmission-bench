//! Where the bench finds its regression gates: `--gates`, then
//! `A2A_BENCH_GATES`, then the repository's `gates/` directory, then none.
//! Only an explicit `--gates` that is missing fails. A path may name one
//! file or a directory of `*.toml` files.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use a2a_bench_score::report::gates::{
    GATES_ENV, GateError, GateSearch, GatesFrom, GatesLocation, REPO_GATES,
};

fn gates(detector: &str) -> String {
    format!("detector = \"{detector}\"\n\n[[gate]]\nname = \"g\"\nmetric = \"recall\"\nmin = 0.5\n")
}

struct Dirs {
    root: tempfile::TempDir,
}

impl Dirs {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    /// A gates file at `name` under the root.
    fn file(&self, name: &str) -> PathBuf {
        let path = self.root.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, gates("d")).unwrap();
        path
    }

    /// A directory under the root.
    fn dir(&self, name: &str) -> PathBuf {
        let path = self.root.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// A path under the root nothing is written to.
    fn missing(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

fn search(flag: Option<&Path>, env: Option<&Path>, repo: &Path) -> GateSearch {
    GateSearch {
        flag: flag.map(Path::to_path_buf),
        env: env.map(Path::to_path_buf),
        repo: repo.to_path_buf(),
    }
}

fn located(search: &GateSearch) -> Option<GatesLocation> {
    search.locate().unwrap()
}

#[test]
fn the_flag_wins_over_everything() {
    let dirs = Dirs::new();
    let flag = dirs.file("flag.toml");
    let found = located(&search(
        Some(&flag),
        Some(&dirs.file("env.toml")),
        &dirs.dir("repo"),
    ));
    assert_eq!(
        found,
        Some(GatesLocation {
            from: GatesFrom::Flag,
            path: flag,
        })
    );
}

#[test]
fn a_missing_flag_path_is_an_error() {
    let dirs = Dirs::new();
    let flag = dirs.missing("nope.toml");
    let error = search(Some(&flag), None, &dirs.dir("repo")).locate().err();
    assert!(
        matches!(&error, Some(GateError::Missing { path }) if path == &flag.display().to_string()),
        "{error:?}"
    );
}

#[test]
fn the_env_var_comes_next() {
    let dirs = Dirs::new();
    let env = dirs.file("env.toml");
    let found = located(&search(None, Some(&env), &dirs.dir("repo")));
    assert_eq!(
        found,
        Some(GatesLocation {
            from: GatesFrom::Env,
            path: env,
        })
    );
}

#[test]
fn then_the_repository_directory() {
    let dirs = Dirs::new();
    let repo = dirs.dir("repo");
    let found = located(&search(None, Some(&dirs.missing("env.toml")), &repo));
    assert_eq!(
        found,
        Some(GatesLocation {
            from: GatesFrom::Repo,
            path: repo,
        }),
        "a missing default falls through, never fails"
    );
}

#[test]
fn no_default_found_means_no_gates() {
    let dirs = Dirs::new();
    let search = search(None, Some(&dirs.missing("env.toml")), &dirs.missing("repo"));
    assert_eq!(located(&search), None);
    let (gates, location) = search.load().unwrap();
    assert!(gates.gates.is_empty());
    assert_eq!(location, None);
}

#[test]
fn a_directory_loads_every_toml_file_in_name_order() {
    let dirs = Dirs::new();
    let repo = dirs.dir("repo");
    std::fs::write(repo.join("b.toml"), gates("b")).unwrap();
    std::fs::write(repo.join("a.toml"), gates("a")).unwrap();
    std::fs::write(repo.join("notes.md"), "not gates").unwrap();
    let (gates, location) = search(None, None, &repo).load().unwrap();
    let detectors: Vec<&str> = gates.gates.iter().map(|g| g.detector.as_str()).collect();
    assert_eq!(detectors, ["a", "b"]);
    assert_eq!(location.map(|l| l.from), Some(GatesFrom::Repo));
}

#[test]
fn a_bad_file_names_its_path() {
    let dirs = Dirs::new();
    let repo = dirs.dir("repo");
    std::fs::write(repo.join("bad.toml"), "[[gate]]\nname = 1\n").unwrap();
    let error = search(None, None, &repo).load().err();
    assert!(
        matches!(&error, Some(GateError::Parse { path, .. }) if path.ends_with("bad.toml")),
        "{error:?}"
    );
}

#[test]
fn the_environment_supplies_the_env_var_and_skips_an_empty_one() {
    let repo = Path::new("/nonexistent/gates");
    let set = GateSearch::new(None, Some("/some/gates.toml".into()), repo.to_path_buf());
    assert_eq!(set.env, Some(PathBuf::from("/some/gates.toml")));
    let empty = GateSearch::new(None, Some(String::new().into()), repo.to_path_buf());
    assert_eq!(empty.env, None);
    assert_eq!(GATES_ENV, "A2A_BENCH_GATES");
}

#[test]
fn the_repository_default_is_the_gates_directory() {
    let repo = Path::new(REPO_GATES);
    assert!(repo.ends_with("gates"));
    assert!(repo.is_dir(), "{}", repo.display());
    let search = GateSearch::from_env(None);
    assert_eq!(search.repo, PathBuf::from(REPO_GATES));
}
