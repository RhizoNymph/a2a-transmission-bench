//! Absolute paths, lexically normalized (crosstalk-eval's reference
//! matcher's `normalize_path`, which the generator used).

/// `path` with empty and `.` segments dropped and `..` applied, always
/// absolute: `/a/./b/../c` is `/a/c`, `..` above the root stays at it.
pub fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}
