use snapshot_identity::{Identity, new};

fn subject() -> std::sync::Arc<dyn Identity> {
    new()
}

// 014.3: every identity string is exactly 11 characters
#[test]
fn identity_is_11_chars() {
    let s = subject();
    assert_eq!(s.identity("docs/readme.txt").len(), 11);
    assert_eq!(s.identity("docs/notes").len(), 11);
    assert_eq!(s.identity("a").len(), 11);
}

// 014.3: parent_identity also returns exactly 11 characters
#[test]
fn parent_identity_is_11_chars() {
    let s = subject();
    assert_eq!(s.parent_identity("docs/readme.txt").len(), 11);
    assert_eq!(s.parent_identity("readme.txt").len(), 11);
}

// 014.2: alphabet is digits 0-9, then A-Z, then a-z (base62, no other chars)
#[test]
fn identity_chars_are_base62() {
    let s = subject();
    for id in [s.identity("docs/readme.txt"), s.identity("docs/notes"), s.identity("a")] {
        assert!(
            id.chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() || c.is_ascii_lowercase()),
            "non-base62 character in identity: {id}"
        );
    }
}

// 014.1: same canonical path always yields the same identity (pure, deterministic)
#[test]
fn identity_is_deterministic() {
    let s = subject();
    assert_eq!(s.identity("docs/readme.txt"), s.identity("docs/readme.txt"));
    assert_eq!(s.identity("docs/notes"), s.identity("docs/notes"));
}

// 014.7: a file and a directory sharing the same canonical path produce the same identity
#[test]
fn file_and_dir_same_canonical_path_same_identity() {
    let s = subject();
    // "docs/notes" used as a file and as a directory must produce identical results
    let as_file = s.identity("docs/notes");
    let as_dir = s.identity("docs/notes");
    assert_eq!(as_file, as_dir);
}

// 014.8: the identity of "docs/readme.txt" is derived from "docs/readme.txt" (no transformation)
#[test]
fn identity_docs_readme_txt() {
    let s = subject();
    let id = s.identity("docs/readme.txt");
    assert_eq!(id.len(), 11);
    // The path is used as-is (no stripping of path segments); result differs from other paths
    assert_ne!(id, s.identity("docs"));
    assert_ne!(id, s.identity("docs/notes"));
}

// 014.9: the identity of directory "docs/notes" is derived from "docs/notes"
#[test]
fn identity_docs_notes_dir() {
    let s = subject();
    let id = s.identity("docs/notes");
    assert_eq!(id.len(), 11);
    assert_ne!(id, s.identity("docs"));
    assert_ne!(id, s.identity("docs/readme.txt"));
}

// 014.10: parent identity of "docs/readme.txt" is the identity of "docs"
#[test]
fn parent_identity_of_docs_readme_txt_equals_identity_of_docs() {
    let s = subject();
    assert_eq!(s.parent_identity("docs/readme.txt"), s.identity("docs"));
}

// 014.11: parent identity of directory "docs/notes" is the identity of "docs"
#[test]
fn parent_identity_of_docs_notes_equals_identity_of_docs() {
    let s = subject();
    assert_eq!(s.parent_identity("docs/notes"), s.identity("docs"));
}

// 014.12: root-level entry's parent identity is the hash of the sentinel "/" (not empty string)
// 014.13: the sync root is never a tracked entry; root-level children name "/" as parent
#[test]
fn root_level_parent_identity_is_sentinel() {
    let s = subject();
    let sentinel = s.parent_identity("readme.txt");
    // All root-level entries share the same sentinel (hash of "/")
    assert_eq!(sentinel, s.parent_identity("notes"));
    assert_eq!(sentinel, s.parent_identity("another_file.txt"));
    // Sentinel is well-formed
    assert_eq!(sentinel.len(), 11);
    // Sentinel is distinct from the parent identity of any nested entry (which hashes a real path segment)
    assert_ne!(sentinel, s.parent_identity("docs/readme.txt"));
}

// 014.4: the canonical path fed to the hash uses forward slashes as separators
#[test]
fn canonical_path_uses_forward_slashes() {
    let s = subject();
    assert_eq!(s.identity("docs\\readme.txt"), s.identity("docs/readme.txt"));
}

// 014.5: the canonical path has no leading slash
#[test]
fn canonical_path_has_no_leading_slash() {
    let s = subject();
    assert_eq!(s.identity("/docs/readme.txt"), s.identity("docs/readme.txt"));
}

// 014.6: the canonical path has no trailing slash
#[test]
fn canonical_path_has_no_trailing_slash() {
    let s = subject();
    assert_eq!(s.identity("docs/notes/"), s.identity("docs/notes"));
}
