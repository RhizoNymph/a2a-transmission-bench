//! Parity with crosstalk at 7f8a2fb: every vector crosstalk computed
//! (`tests/fixtures/crosstalk-7f8a2fb-resource-vectors.json`) gives the
//! same result here, errors included; and `canonicalize` keeps every
//! canonical output as it is.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_format::resource::Resource;
use a2a_bench_resource::{
    canonical_remote, canonical_url, canonicalize, kind, normalized_url, repository,
};
use common::{render_as_locator, render_locator, vectors};
use serde_json::Value;

/// A crosstalk result (`{"ok": locator}` or `{"err": "<Debug>"}`) in the
/// spec notation.
fn expected(result: &Value) -> String {
    match (result.get("ok"), result.get("err")) {
        (Some(locator), None) => render_locator(locator),
        (None, Some(error)) => format!("error {}", error.as_str().unwrap()),
        _ => panic!("a result: {result}"),
    }
}

fn got<E: std::fmt::Debug>(result: &Result<Resource, E>) -> String {
    match result {
        Ok(resource) => render_as_locator(resource),
        Err(error) => format!("error {error:?}"),
    }
}

fn cases(vectors: &Value) -> &Vec<Value> {
    vectors["vectors"].as_array().unwrap()
}

/// Every vector's mismatches, one line each.
fn mismatches(check: impl Fn(&str, &Value) -> Option<String>) -> Vec<String> {
    let vectors = vectors();
    cases(&vectors)
        .iter()
        .filter_map(|vector| check(vector["input"].as_str().unwrap(), vector))
        .collect()
}

fn assert_none(what: &str, failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} {what} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn there_are_enough_vectors() {
    let vectors = vectors();
    assert_eq!(vectors["crosstalk"], "7f8a2fb");
    assert!(cases(&vectors).len() >= 500);
}

#[test]
fn canonical_url_equals_from_url() {
    let failures = mismatches(|input, vector| {
        let (want, have) = (expected(&vector["url"]), got(&canonical_url(input)));
        (want != have).then(|| format!("{input:?}: crosstalk {want:?}, bench {have:?}"))
    });
    assert_none("canonical_url", &failures);
}

#[test]
fn normalized_url_equals_url_locator() {
    let failures = mismatches(|input, vector| {
        let (want, have) = (expected(&vector["plain"]), got(&normalized_url(input)));
        (want != have).then(|| format!("{input:?}: crosstalk {want:?}, bench {have:?}"))
    });
    assert_none("normalized_url", &failures);
}

#[test]
fn canonical_remote_equals_from_remote() {
    let failures = mismatches(|input, vector| {
        let want = match &vector["remote"] {
            Value::Null => None,
            locator => Some(render_locator(locator)),
        };
        let have = canonical_remote(input)
            .ok()
            .map(|repo| render_as_locator(&Resource::Repository(repo)));
        (want != have).then(|| format!("{input:?}: crosstalk {want:?}, bench {have:?}"))
    });
    assert_none("canonical_remote", &failures);
}

#[test]
fn kind_equals_ai_village_kind() {
    let failures = mismatches(|input, vector| {
        let want = vector["kind"].as_str();
        let have = canonical_url(input)
            .ok()
            .and_then(|resource| kind(&resource))
            .map(|kind| kind.as_str());
        (want != have).then(|| format!("{input:?}: crosstalk {want:?}, bench {have:?}"))
    });
    assert_none("kind", &failures);
}

#[test]
fn repository_equals_locator_repository() {
    let vectors = vectors();
    let failures: Vec<String> = vectors["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|case| {
            let part = |key: &str| case[key].as_str().unwrap();
            let want = expected(&case["repository"]);
            let have =
                got(&repository(part("host"), part("owner"), part("name"))
                    .map(Resource::Repository));
            (want != have).then(|| format!("{case}: crosstalk {want:?}, bench {have:?}"))
        })
        .collect();
    assert!(vectors["repositories"].as_array().unwrap().len() >= 40);
    assert_none("repository", &failures);
}

/// Whether `repository` keeps the repository `resource` holds (or it holds
/// none). It does not for parts crosstalk folds only part of the way in
/// one pass (`www.www.`, an upper-case `.GIT`, `.git.git`), which
/// `canonicalize` folds the rest of the way.
fn repository_is_fixed(resource: &Resource) -> bool {
    let repo = match resource {
        Resource::Repository(repo)
        | Resource::RepoFile {
            repository: repo, ..
        }
        | Resource::Thread {
            repository: repo, ..
        }
        | Resource::Collection {
            repository: repo, ..
        } => repo,
        Resource::Url(_) | Resource::File { .. } | Resource::Opaque { .. } => return true,
    };
    repository(&repo.host, &repo.owner, &repo.name).as_ref() == Ok(repo)
}

/// Canonical outputs are fixed points of `canonicalize`, and a `Url` of
/// the input canonicalizes as `canonical_url` does, except where
/// [`repository_is_fixed`] says crosstalk's one pass stopped short.
#[test]
fn canonicalize_keeps_canonical_outputs() {
    let failures = mismatches(|input, _| {
        let mut failures = Vec::new();
        if let Ok(resource) = canonical_url(input)
            && repository_is_fixed(&resource)
        {
            if canonicalize(&resource) != resource {
                failures.push(format!("canonical_url {resource:?}"));
            }
            let as_url = canonicalize(&Resource::Url(input.to_owned()));
            if as_url != resource {
                failures.push(format!("Url({input:?}) gives {as_url:?}, not {resource:?}"));
            }
        }
        if let Ok(normalized) = normalized_url(input) {
            let again = canonicalize(&normalized);
            if canonicalize(&again) != again {
                failures.push(format!("normalized_url {normalized:?} is not stable"));
            }
        }
        if let Ok(repo) = canonical_remote(input) {
            let resource = Resource::Repository(repo);
            if repository_is_fixed(&resource) && canonicalize(&resource) != resource {
                failures.push(format!("canonical_remote {resource:?}"));
            }
        }
        (!failures.is_empty()).then(|| format!("{input:?}: {}", failures.join("; ")))
    });
    assert_none("canonicalize", &failures);
}

/// Where crosstalk's pass stops short, `canonicalize` still reaches a fixed
/// point, and `canonicalize` of the input's `Url` reaches the same one.
#[test]
fn canonicalize_finishes_what_one_pass_leaves() {
    let failures = mismatches(|input, _| {
        let resource = canonical_url(input).ok()?;
        let once = canonicalize(&resource);
        let fixed = canonicalize(&once) == once
            && repository_is_fixed(&once)
            && canonicalize(&Resource::Url(input.to_owned())) == once;
        (!fixed).then(|| format!("{input:?}: {resource:?} -> {once:?}"))
    });
    assert_none("fixed point", &failures);
}
