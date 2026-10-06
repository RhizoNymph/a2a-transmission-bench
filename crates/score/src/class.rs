//! Evidence classes: what a prediction rests on, and the class a label's
//! need puts it in. Class picks the report row; it never decides alignment.

use serde::{Deserialize, Serialize};

use a2a_bench_format::labels::{MatchClass, MatchNeed};

/// A content match of a class (strongest first), or only an access
/// pattern, still suspected or discarded. A label's need is always a
/// content class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Exact,
    Normalized,
    Decoded,
    Semantic,
    /// A co-access only, with no content match yet.
    Suspected,
    /// A co-access that expired without content evidence.
    Discarded,
}

impl EvidenceClass {
    /// The match class of content evidence; `None` for access-only.
    pub const fn content(self) -> Option<MatchClass> {
        match self {
            Self::Exact => Some(MatchClass::Exact),
            Self::Normalized => Some(MatchClass::Normalized),
            Self::Decoded => Some(MatchClass::Decoded),
            Self::Semantic => Some(MatchClass::Semantic),
            Self::Suspected | Self::Discarded => None,
        }
    }

    pub const fn is_content(self) -> bool {
        self.content().is_some()
    }
}

impl From<MatchClass> for EvidenceClass {
    fn from(class: MatchClass) -> Self {
        match class {
            MatchClass::Exact => Self::Exact,
            MatchClass::Normalized => Self::Normalized,
            MatchClass::Decoded => Self::Decoded,
            MatchClass::Semantic => Self::Semantic,
        }
    }
}

/// The class a label with `need` is counted in: undecodable text counts as
/// decoded, unobserved text by the class it arrives in.
pub fn need_class(need: &MatchNeed) -> MatchClass {
    match need {
        MatchNeed::Exact => MatchClass::Exact,
        MatchNeed::Normalized => MatchClass::Normalized,
        MatchNeed::Decoded { .. } | MatchNeed::Undecodable { .. } => MatchClass::Decoded,
        MatchNeed::Semantic => MatchClass::Semantic,
        MatchNeed::Unobserved { arrival, .. } => *arrival,
    }
}
