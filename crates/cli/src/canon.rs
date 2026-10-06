//! The scorer's canonicalisation seam filled with the bench's resource
//! canonicaliser: labels' and predictions' channel resources are compared
//! after `a2a_bench_resource::canonicalize`, so a detector that writes a raw
//! URL or remote is canonicalised by the bench, not by itself (design §3.6).

use std::borrow::Cow;

use a2a_bench_format::resource::Resource;
use a2a_bench_score::Canonicalize;

/// [`Canonicalize`] by [`a2a_bench_resource::canonicalize`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceCanon;

impl Canonicalize for ResourceCanon {
    fn canonical<'r>(&self, resource: &'r Resource) -> Cow<'r, Resource> {
        Cow::Owned(a2a_bench_resource::canonicalize(resource))
    }
}
