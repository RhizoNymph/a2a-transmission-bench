//! The resource canonicalisation seam. Alignment compares a channel label's
//! resource with a prediction's after passing both through a
//! [`Canonicalize`]; the bench's canonicaliser (`a2a-bench-resource`) plugs
//! in here. [`AsGiven`] compares resources exactly as written.

use std::borrow::Cow;

use a2a_bench_format::resource::Resource;

/// Turns a resource into its canonical form. Two resources name the same
/// channel exactly when their canonical forms are equal.
pub trait Canonicalize {
    fn canonical<'r>(&self, resource: &'r Resource) -> Cow<'r, Resource>;
}

/// Resources as given: no canonicalisation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AsGiven;

impl Canonicalize for AsGiven {
    fn canonical<'r>(&self, resource: &'r Resource) -> Cow<'r, Resource> {
        Cow::Borrowed(resource)
    }
}

/// Whether `a` and `b` are the same resource under `canon`.
pub fn same_resource(canon: &dyn Canonicalize, a: &Resource, b: &Resource) -> bool {
    canon.canonical(a) == canon.canonical(b)
}
