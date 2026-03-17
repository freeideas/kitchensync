use xxhash_rust::xxh64::xxh64;

/// Hash a path using xxHash64, returning 8 bytes.
/// Paths are normalized: forward slashes, no leading slash, trailing slash for directories.
pub fn hash_path(path: &str) -> [u8; 8] {
    let hash = xxh64(path.as_bytes(), 0);
    hash.to_le_bytes()
}

/// Normalize a path for hashing.
/// - Forward slashes as separators
/// - No leading slash
/// - Trailing slash for directories
pub fn normalize_path(path: &str, is_dir: bool) -> String {
    let mut normalized = path.replace('\\', "/");

    // Remove leading slash
    while normalized.starts_with('/') {
        normalized = normalized[1..].to_string();
    }

    // Handle trailing slash
    if is_dir {
        if !normalized.ends_with('/') && !normalized.is_empty() {
            normalized.push('/');
        }
    } else {
        while normalized.ends_with('/') {
            normalized.pop();
        }
    }

    normalized
}

/// Get the parent path (with trailing slash) for a given normalized path.
/// For "docs/readme.txt" returns "docs/"
/// For "readme.txt" (at root) returns "/"
pub fn parent_path(normalized_path: &str) -> String {
    // Remove trailing slash if present (for directories)
    let path = normalized_path.trim_end_matches('/');

    if let Some(pos) = path.rfind('/') {
        format!("{}/", &path[..pos])
    } else {
        "/".to_string()
    }
}

/// Get the basename of a path.
pub fn basename(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if let Some(pos) = path.rfind('/') {
        path[pos + 1..].to_string()
    } else {
        path.to_string()
    }
}
