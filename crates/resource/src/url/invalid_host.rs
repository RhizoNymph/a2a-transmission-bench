//! A `scheme://` URL whose host the parser refuses (IDNA, a forbidden
//! domain character, a bad IPv4 or IPv6 address) is not dropped: it is an
//! opaque key, so every agent that uses the same invalid URL reaches one
//! resource. Such text never becomes a URL resource.

/// The tool of the `Resource::Opaque` an invalid-host URL becomes. Angle
/// brackets keep it apart from any real tool's name.
pub const INVALID_HOST_TOOL: &str = "<url>";

/// The key of `text` when `error` is a host error and `text` is
/// `scheme://…`: trimmed, scheme and host part lower case (Unicode), user
/// info and fragment dropped. `None` otherwise.
pub(crate) fn key(text: &str, error: url::ParseError) -> Option<String> {
    use url::ParseError::{
        IdnaError, InvalidDomainCharacter, InvalidIpv4Address, InvalidIpv6Address,
    };
    if !matches!(
        error,
        IdnaError | InvalidDomainCharacter | InvalidIpv4Address | InvalidIpv6Address
    ) {
        return None;
    }
    let (scheme, rest) = text.split_once("://")?;
    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_ok {
        return None;
    }
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let authority_end = rest.find(['/', '?', '\\']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    Some(format!(
        "{}://{}{tail}",
        scheme.to_ascii_lowercase(),
        host.to_lowercase()
    ))
}
