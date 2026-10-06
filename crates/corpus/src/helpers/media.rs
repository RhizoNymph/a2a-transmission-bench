//! A media part's kind from what a source says it holds: a MIME type
//! (`image/png`, `application/pdf; name=x`), or a bare family name
//! (`image`, `audio`, `document`, `pdf`). The format keeps only the kind.

use a2a_bench_format::message::MediaKind;

/// The kind of media `media_type` names: `image/*` → image, `audio/*` →
/// audio, PDFs, office and text documents → document, anything else
/// (video, archives, an unknown name) → other. Case and MIME parameters are
/// ignored.
pub fn media_kind(media_type: &str) -> MediaKind {
    let essence = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let (family, subtype) = essence.split_once('/').unwrap_or((essence.as_str(), ""));
    match family {
        "image" => MediaKind::Image,
        "audio" => MediaKind::Audio,
        "text" | "document" | "pdf" => MediaKind::Document,
        "application" if is_document(subtype) => MediaKind::Document,
        _ => MediaKind::Other,
    }
}

/// Whether an `application/*` subtype is a document format.
fn is_document(subtype: &str) -> bool {
    matches!(
        subtype,
        "pdf" | "msword" | "rtf" | "epub+zip" | "x-latex" | "vnd.ms-excel" | "vnd.ms-powerpoint"
    ) || subtype.starts_with("vnd.openxmlformats-officedocument.")
        || subtype.starts_with("vnd.oasis.opendocument.")
}
