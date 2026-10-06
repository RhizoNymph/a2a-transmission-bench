//! Text the matcher compares: folding, classification of a hit, decoding of
//! encoded tokens, k-gram shingles and opaque blobs. Pure functions over
//! strings; nothing here knows about worlds or exchanges.

pub mod classify;
pub mod decode;
pub mod fold;
pub mod opaque;
pub mod shingle;
