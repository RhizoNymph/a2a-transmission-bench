//! A URL written without its scheme (`www.example.com`,
//! `example.com:8080/a`): a model often leaves it off, and the tool fetches
//! the page all the same.

/// File extensions that are also TLDs and that a bare file name ends in
/// (`README.md`, `main.rs`). Text that is only such a name, with no `www.`,
/// port or path, is a file name, not a host.
const FILE_EXTENSIONS: &[&str] = &[
    "bash", "bin", "cfg", "conf", "cpp", "css", "csv", "dart", "doc", "docx", "env", "exe", "gif",
    "go", "gz", "hpp", "htm", "html", "ini", "ipynb", "java", "jpeg", "jpg", "js", "json", "jsx",
    "kt", "lock", "log", "lua", "md", "mdx", "pdf", "php", "pl", "png", "pptx", "py", "rb", "rs",
    "rst", "sh", "so", "sql", "svg", "swift", "tar", "tex", "toml", "ts", "tsv", "tsx", "txt",
    "vue", "wasm", "xls", "xlsx", "xml", "yaml", "yml", "zip",
];

/// `text` when it is a bare `host[:port][/path…]`: no `://`, no
/// whitespace, a host that is a domain (dot-separated labels of letters,
/// digits and inner hyphens, at most 63 characters each and 253 in all,
/// ending in an alphabetic label of two or more characters; a trailing dot
/// allowed) and an optional numeric port of one to five digits. Not a
/// bare file name.
pub(crate) fn host(text: &str) -> Option<&str> {
    if text.is_empty() || text.contains("://") || text.chars().any(char::is_whitespace) {
        return None;
    }
    let authority_end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let authority = text.get(..authority_end)?;
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if let Some(port) = port
        && (port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let domain = host.strip_suffix('.').unwrap_or(host);
    if domain.len() > 253 {
        return None;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    let tld = labels.last().copied().filter(|_| labels.len() >= 2)?;
    let label_ok = |label: &str| {
        !label.is_empty()
            && label.chars().count() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    if !labels.iter().all(|label| label_ok(label))
        || tld.chars().count() < 2
        || !tld.chars().all(char::is_alphabetic)
    {
        return None;
    }
    let only_a_name = port.is_none() && authority_end == text.len();
    if only_a_name
        && !domain.to_lowercase().starts_with("www.")
        && FILE_EXTENSIONS.contains(&tld.to_lowercase().as_str())
    {
        return None;
    }
    Some(text)
}
