//! Helpers for the CLI's end-to-end tests: the `a2a-bench` and
//! `a2a-reference` binaries, the converters' synthetic fixtures, scratch
//! directories inside and outside any git repository.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// The `a2a-bench` binary under test.
pub fn bench() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_a2a-bench"))
}

/// The workspace root.
pub fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A converter's synthetic fixture directory, e.g. `salt/tests/fixtures/salt`.
pub fn fixture(relative: &str) -> PathBuf {
    workspace().join("crates/datasets").join(relative)
}

/// The `a2a-reference` binary: built beside `a2a-bench` (same target dir
/// and profile). A workspace `cargo test` builds it already; otherwise it
/// is built once here with the cargo running the tests.
pub fn reference() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let bench = bench();
            let path =
                bench.with_file_name(format!("a2a-reference{}", std::env::consts::EXE_SUFFIX));
            if path.is_file() {
                return path;
            }
            let profile_dir = bench.parent().unwrap();
            let target = profile_dir.parent().unwrap();
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut build = Command::new(cargo);
            build
                .current_dir(workspace())
                .args([
                    "build",
                    "-p",
                    "a2a-bench-reference",
                    "--bin",
                    "a2a-reference",
                ])
                .arg("--target-dir")
                .arg(target);
            if profile_dir
                .file_name()
                .is_some_and(|name| name == "release")
            {
                build.arg("--release");
            }
            let status = build.status().unwrap();
            assert!(status.success(), "building a2a-reference failed");
            assert!(path.is_file(), "{} missing after the build", path.display());
            path
        })
        .clone()
}

/// Runs `a2a-bench args…` with no gates from the environment.
pub fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(bench())
        .args(args)
        .env_remove("A2A_BENCH_GATES")
        .env("RUST_LOG", "warn")
        .output()
        .unwrap()
}

/// `run` with string arguments.
pub fn run_str(args: &[&str]) -> Output {
    let args: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    run(&args)
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Asserts the exit code, showing both streams otherwise.
pub fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

/// A scratch directory (under TMPDIR, inside the worktree when TMPDIR is
/// `target/tmp`).
pub fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn in_repository(path: &Path) -> bool {
    a2a_bench_corpus::split::enclosing_repository(path)
        .unwrap()
        .is_some()
}

/// A scratch directory outside every git repository, for holdout outputs:
/// `A2A_BENCH_TEST_OUTSIDE_REPO`, else the directory holding the checkout,
/// else the system temp dir. `None` when each is inside a repository.
pub fn outside_repository() -> Option<tempfile::TempDir> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("A2A_BENCH_TEST_OUTSIDE_REPO") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(root) = workspace().canonicalize()
        && let Some(parent) = root.parent()
    {
        candidates.push(parent.join(".a2a-bench-test-tmp"));
    }
    candidates.push(std::env::temp_dir());
    for base in candidates {
        if std::fs::create_dir_all(&base).is_err() || in_repository(&base) {
            continue;
        }
        if let Ok(dir) = tempfile::tempdir_in(&base) {
            return Some(dir);
        }
    }
    None
}

/// Copies the tree at `from` into `to` (created).
pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The manifest of the export in `dir`, as JSON.
pub fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap()
}

/// Every file under `dir`, relative, with its bytes, sorted.
pub fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.push((
                    path.strip_prefix(base).unwrap().to_path_buf(),
                    std::fs::read(&path).unwrap(),
                ));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A gates file for the reference detector with one recall gate on
/// `dataset` at `min`.
pub fn gates(dir: &Path, dataset: &str, min: f64) -> PathBuf {
    let path = dir.join(format!("gates-{min}.toml"));
    std::fs::write(
        &path,
        format!(
            "detector = \"reference\"\n\n[[gate]]\nname = \"{dataset}: every label\"\ndataset = \"{dataset}\"\nmetric = \"recall\"\nmin = {min}\n"
        ),
    )
    .unwrap();
    path
}
