use xxhash_rust::xxh64::xxh64;

const BASE62_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// xxHash64 (seed 0) → base62 encoded, 11 characters, zero-padded.
pub fn path_hash(path: &str) -> String {
    let h = xxh64(path.as_bytes(), 0);
    base62_encode(h)
}

fn base62_encode(mut val: u64) -> String {
    let mut buf = [b'0'; 11];
    for i in (0..11).rev() {
        buf[i] = BASE62_CHARS[(val % 62) as usize];
        val /= 62;
    }
    String::from_utf8(buf.to_vec()).unwrap()
}

/// Compute the snapshot id for a file path (forward slashes, no leading slash).
pub fn file_id(rel_path: &str) -> String {
    path_hash(rel_path)
}

/// Compute the snapshot id for a directory path (with trailing slash).
pub fn dir_id(rel_path: &str) -> String {
    let p = if rel_path.ends_with('/') {
        rel_path.to_string()
    } else {
        format!("{}/", rel_path)
    };
    path_hash(&p)
}

/// Compute parent_id: hash of parent path with trailing slash.
/// Root entries use hash of "/".
pub fn parent_id(rel_path: &str) -> String {
    let rel_path = rel_path.trim_end_matches('/');
    if let Some(pos) = rel_path.rfind('/') {
        path_hash(&format!("{}/", &rel_path[..pos]))
    } else {
        path_hash("/")
    }
}

/// Compute the snapshot id for an entry (file or directory).
pub fn entry_id(rel_path: &str, is_dir: bool) -> String {
    if is_dir {
        dir_id(rel_path)
    } else {
        file_id(rel_path)
    }
}
