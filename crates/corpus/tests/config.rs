//! `datasets.toml`: parsing, defaults, `~` expansion, dataset roots.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use a2a_bench_corpus::config::{ConfigError, DEFAULT_DATASETS, DatasetsConfig, expand};
use common::ok;

#[test]
fn the_shipped_file_matches_the_defaults() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../datasets.toml");
    let shipped = ok(DatasetsConfig::load(&path));
    assert_eq!(shipped, DatasetsConfig::default());
    assert_eq!(shipped.datasets.len(), DEFAULT_DATASETS.len());
    assert!(
        shipped
            .datasets
            .values()
            .all(|d| d.pinned_revision().is_none())
    );
}

#[test]
fn paths_expand_home_and_join_the_root() {
    let home = Path::new("/home/u");
    assert_eq!(expand("~/Data", Some(home)), Path::new("/home/u/Data"));
    assert_eq!(expand("~", Some(home)), Path::new("/home/u"));
    assert_eq!(expand("~/Data", None), Path::new("~/Data"));
    assert_eq!(expand("/abs", Some(home)), Path::new("/abs"));
    let config = DatasetsConfig::default();
    assert_eq!(
        ok(config.dataset_root("salt", Some(home))),
        Path::new("/home/u/Data/ai/agents/salt-nlp")
    );
    assert!(matches!(
        config.dataset_root("nope", Some(home)),
        Err(ConfigError::UnknownDataset(_))
    ));
}

#[test]
fn a_config_names_paths_and_revisions() {
    let text = r#"
root = "/data"

[datasets.salt]
path = "salt-nlp"
revision = "abc123"

[datasets.local]
path = "/elsewhere/local"
"#;
    let config = ok(DatasetsConfig::parse(text, "t.toml"));
    assert_eq!(
        ok(config.dataset_root("salt", None)),
        Path::new("/data/salt-nlp")
    );
    assert_eq!(
        ok(config.dataset_root("local", None)),
        Path::new("/elsewhere/local")
    );
    assert_eq!(ok(config.dataset("salt")).pinned_revision(), Some("abc123"));
    assert_eq!(ok(config.dataset("local")).pinned_revision(), None);
    assert!(matches!(
        DatasetsConfig::parse("root = 1", "t.toml"),
        Err(ConfigError::Parse { .. })
    ));
    assert!(matches!(
        DatasetsConfig::parse("root = \"/\"\nextra = 1", "t.toml"),
        Err(ConfigError::Parse { .. })
    ));
}
