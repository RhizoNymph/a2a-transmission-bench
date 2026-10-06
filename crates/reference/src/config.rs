//! Matcher parameters.

use serde::Serialize;

/// The default boilerplate cutoff: crosstalk L4's `IndexSettings::cutoff`
/// default (a fingerprint observed in more live texts than this is
/// boilerplate), so the reference and L4 call the same text boilerplate.
pub const MAX_POSTINGS: usize = 50;

/// Matcher parameters. Every field is part of the detector's variant
/// (`config_digest` in the predictions header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReferenceConfig {
    /// Shingle length in folded bytes.
    pub k: usize,
    /// The shortest span (and match) in folded bytes; at least `k`.
    pub min_span: usize,
    /// The shortest encoded token worth decoding, in raw bytes.
    pub min_decoded: usize,
    /// The fewest letters and digits a span or match must hold, so a window
    /// of mostly JSON syntax (`"}]","reasoning":"the `) never counts.
    pub min_word_chars: usize,
    /// The most distinct originated spans a shingle may be posted for; one
    /// more makes it boilerplate (never indexed or looked up again). The
    /// default is [`MAX_POSTINGS`].
    pub max_postings: usize,
}

impl Default for ReferenceConfig {
    fn default() -> Self {
        Self {
            k: 24,
            min_span: 24,
            min_decoded: 16,
            min_word_chars: 20,
            max_postings: MAX_POSTINGS,
        }
    }
}

impl ReferenceConfig {
    /// The config the matcher runs with: `min_span` raised to `k`.
    pub fn effective(self) -> Self {
        Self {
            min_span: self.min_span.max(self.k),
            ..self
        }
    }
}
