//! The bench repository the binary was built from: its commit, its
//! `datasets.toml`, its `splits/` directory, and the converter identity an
//! export's manifest records.

use std::path::PathBuf;

use a2a_bench_format::manifest::Converter;

/// The bench commit at build time (`git rev-parse HEAD`), or `unknown`.
pub const BENCH_GIT: &str = env!("A2A_BENCH_GIT");

/// The converters' version: the workspace's package version.
pub const CONVERTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The repository root of the source checkout the binary was built from.
pub fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The repository's `datasets.toml`.
pub fn datasets_config() -> PathBuf {
    root().join("datasets.toml")
}

/// The repository's `splits/` directory (dev lists and holdout commits).
pub fn splits_dir() -> PathBuf {
    root().join("splits")
}

/// What an export's manifest records as its converter.
pub fn converter() -> Converter {
    Converter {
        version: CONVERTER_VERSION.to_owned(),
        git: BENCH_GIT.to_owned(),
    }
}
