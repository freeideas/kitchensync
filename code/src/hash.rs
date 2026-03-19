use xxhash_rust::xxh64::xxh64;

/// Hash a relative path to 8 bytes using xxHash64 with seed 0.
/// Paths use forward slashes, no leading slash.
/// Directories have trailing slash.
pub fn path_hash(path: &str) -> [u8; 8] {
    xxh64(path.as_bytes(), 0).to_be_bytes()
}

/// Hash for the parent of a given path.
/// Parent of "docs/readme.txt" -> hash of "docs/"
/// Parent of root entries -> hash of "/"
pub fn parent_hash(path: &str) -> [u8; 8] {
    let parent = parent_path(path);
    path_hash(&parent)
}

/// Get the parent path string (with trailing slash).
/// "docs/readme.txt" -> "docs/"
/// "readme.txt" -> "/"
/// "docs/notes/" -> "docs/"
fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => trimmed[..=i].to_string(),
        None => "/".to_string(),
    }
}

/// Get basename from a path.
pub fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parent_path() {
        assert_eq!(parent_path("docs/readme.txt"), "docs/");
        assert_eq!(parent_path("readme.txt"), "/");
        assert_eq!(parent_path("docs/notes/"), "docs/");
        assert_eq!(parent_path("a/b/c"), "a/b/");
    }
}
