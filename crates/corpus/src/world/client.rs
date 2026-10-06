//! What a proxy would observe of a corpus agent: a stable synthetic
//! credential and the vendor its model names.
//!
//! Datasets carry no wire headers, so each agent gets one stable synthetic
//! API-key credential: crosstalk-eval's digest of its key, so it is the
//! same on every run, different for every agent, and the same bytes
//! crosstalk's adapter needs to rebuild ct-eval's `ClientContext`
//! (`CredentialHash::from_keyed_digest(SecretVersion(0), digest)`).
//!
//! The digest is
//!
//! ```text
//! BLAKE3("crosstalk-eval/v1" 0x00 "credential" 0x00 dataset 0x00 world 0x00 agent)
//! ```
//!
//! and the credential text is `k:` followed by its 64 lower-case hex digits.

use std::fmt;

use a2a_bench_format::ids::{AgentKey, DOMAIN, DatasetId, Digest, WorldKey};

/// The prefix of a credential's text.
pub const CREDENTIAL_PREFIX: &str = "k:";

/// An opaque credential fingerprint, as `Client::credential` carries it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Credential(String);

impl Credential {
    /// The synthetic credential of `agent` in `world` of `dataset`.
    pub fn synthetic(dataset: &DatasetId, world: &WorldKey, agent: &AgentKey) -> Self {
        Self::of_digest(&credential_digest(dataset, world, agent))
    }

    /// The credential text of a digest: `k:<hex>`.
    pub fn of_digest(digest: &Digest) -> Self {
        Self(format!("{CREDENTIAL_PREFIX}{}", digest.to_hex()))
    }

    /// A recorded credential fingerprint, kept as it is (several agents may
    /// share one, as demo-swarm's key groups do).
    pub fn recorded(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// crosstalk-eval's credential digest for `agent` (see the module docs).
pub fn credential_digest(dataset: &DatasetId, world: &WorldKey, agent: &AgentKey) -> Digest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN.as_bytes());
    for field in [
        "credential",
        dataset.as_str(),
        world.as_str(),
        agent.as_str(),
    ] {
        hasher.update(&[0]);
        hasher.update(field.as_bytes());
    }
    Digest::from_bytes(*hasher.finalize().as_bytes())
}

/// The vendor a provider-prefixed model name (`gemini/…`,
/// `bedrock/converse/…anthropic…`, `openai/…`) points at: `anthropic`,
/// `google`, `openai`, or the name's first path segment, lower case.
/// crosstalk-eval's `vendor_of`, written as text (its `Vendor::OpenAi` is
/// `openai`, `Vendor::Other(p)` is `p`).
pub fn vendor_of(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    if lower.contains("anthropic") || lower.contains("claude") {
        "anthropic".to_owned()
    } else if lower.starts_with("gemini") || lower.contains("gemini") || lower.contains("gemma") {
        "google".to_owned()
    } else if lower.starts_with("openai/gpt") || lower.starts_with("gpt") {
        "openai".to_owned()
    } else {
        lower.split('/').next().unwrap_or(&lower).to_owned()
    }
}
