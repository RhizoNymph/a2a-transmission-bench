//! Absolute POSIX paths, resolved lexically: `.`, `..`, repeated and
//! trailing `/` are resolved, symlinks are not followed, and `..` above the
//! root stays at the root.

/// `text`, which must start with `/` and hold no NUL, normalized. `None`
/// otherwise.
pub(crate) fn absolute(text: &str) -> Option<String> {
    if !text.starts_with('/') || text.contains('\0') {
        return None;
    }
    let mut segments: Vec<&str> = Vec::new();
    for segment in text.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            name => segments.push(name),
        }
    }
    if segments.is_empty() {
        return Some("/".to_owned());
    }
    Some(
        segments
            .iter()
            .map(|segment| format!("/{segment}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::absolute;

    #[test]
    fn resolves_lexically() {
        assert_eq!(absolute("/a/./b/../c//d/").as_deref(), Some("/a/c/d"));
        assert_eq!(absolute("/../..").as_deref(), Some("/"));
        assert_eq!(absolute("/").as_deref(), Some("/"));
        assert_eq!(absolute("a/b"), None);
        assert_eq!(absolute("/a\0"), None);
        assert_eq!(absolute(""), None);
    }
}
