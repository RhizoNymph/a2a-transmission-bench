//! Records the bench commit (`git rev-parse HEAD`) as `A2A_BENCH_GIT` for
//! the manifest's `converter.git`; `unknown` outside a git checkout.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=A2A_BENCH_GIT={commit}");
    println!("cargo:rerun-if-changed=build.rs");
    // Rebuild when HEAD moves: HEAD itself, the branch it names, packed refs.
    let mut watched = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(reference);
    }
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}
