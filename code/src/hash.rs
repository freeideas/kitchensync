use xxhash_rust::xxh64::xxh64;

pub fn path_id(path: &str) -> [u8; 8] {
    xxh64(path.as_bytes(), 0).to_be_bytes()
}

pub fn parent_path(path: &str) -> String {
    // path is like "docs/readme.txt" or "docs/notes/"
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => format!("{}/", &trimmed[..pos + 1]),
        None => "/".to_string(),
    }
}

pub fn snapshot_path(dir_path: &str, basename: &str, is_dir: bool) -> String {
    let base = dir_path.trim_end_matches('/');
    if base.is_empty() {
        if is_dir {
            format!("{}/", basename)
        } else {
            basename.to_string()
        }
    } else if is_dir {
        format!("{}/{}/", base, basename)
    } else {
        format!("{}/{}", base, basename)
    }
}
