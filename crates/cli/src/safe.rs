//! Error descriptions that never carry dataset text.
//!
//! swarm-traces labels are real attack payloads, so no output of the CLI
//! may hold label or message text, for any dataset. The format's check
//! errors name only ids, ranges and counts; a JSON row error may quote the
//! row (serde names unknown variants and fields), so it is reduced to its
//! line number.

use a2a_bench_format::jsonl::ReadError;

/// A [`ReadError`] described by kind, line and counts only.
pub fn read_error(error: &ReadError) -> String {
    match error {
        ReadError::Json { line, .. } => format!("line {line}: the row does not parse"),
        ReadError::Io { line, source } => format!("reading line {line}: {}", source.kind()),
        ReadError::NotHeader { .. } => "line 1 is not the header".to_owned(),
        ReadError::WrongFile { expected, .. } => {
            format!("the header names another file kind than {expected}")
        }
        other @ (ReadError::MissingHeader
        | ReadError::WrongFormat { .. }
        | ReadError::RowOutsideWorld { .. }
        | ReadError::SecondHeader { .. }
        | ReadError::DuplicateWorld { .. }
        | ReadError::Truncated
        | ReadError::Counts { .. }
        | ReadError::Digest { .. }
        | ReadError::AfterTrailer { .. }) => other.to_string(),
    }
}
