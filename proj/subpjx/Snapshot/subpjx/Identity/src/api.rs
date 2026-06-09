//! Public specification for the Identity subproject.
//!
//! Identity turns a tracked entry's relative path into the stable string that
//! names its row in the snapshot database, and into the string that names its
//! parent's row. It is a pure, dependency-free primitive: the same path always
//! produces the same identity, with no I/O, no clock, and no shared mutable
//! state.

/// Computes the stable row id and parent-row id of a tracked entry from its
/// relative path.
///
/// An identity is the xxHash64 (seed 0) of a canonical relative path, base62-
/// encoded with digits `0-9`, then uppercase `A-Z`, then lowercase `a-z`, and
/// zero-padded on the left to exactly 11 characters.
///
/// Canonicalization is applied before hashing in every operation: segments are
/// separated by forward slashes, and any leading or trailing slash is removed.
/// The same canonical form is used for a file and a directory, so an entry's
/// type never affects its identity; the byte size column, not the identity,
/// is what later distinguishes a directory from a file.
///
/// The computation is pure and deterministic and reaches no filesystem, so it
/// raises no transport or database errors. It is given a relative path the
/// caller has already chosen to track; it does not validate that the path
/// exists or decide whether it should be tracked. It never produces an identity
/// for the sync root as a tracked entry; only the root's children are tracked.
pub trait Identity: Send + Sync {
    /// Returns the identity of the entry at `path`.
    ///
    /// `path` is first put into canonical form (forward slashes, no leading or
    /// trailing slash), then hashed with xxHash64 using seed 0, and the 64-bit
    /// result is base62-encoded and zero-padded to 11 characters.
    ///
    /// The result is always exactly 11 base62 characters. Identical canonical
    /// paths always yield identical identities, and a file and a directory that
    /// share a canonical path share an identity.
    ///
    /// Examples: the identity of `docs/readme.txt` is the hash of
    /// `docs/readme.txt`; the identity of the directory `docs/notes` is the
    /// hash of `docs/notes`.
    fn identity(&self, path: &str) -> String;

    /// Returns the identity of the parent directory of the entry at `path`.
    ///
    /// This is the same hash applied to the canonical path with its last
    /// segment removed. A root-level entry, whose canonical path has no parent
    /// segment, takes the identity of the sentinel path `/` -- never the hash of
    /// an empty string.
    ///
    /// The result is always exactly 11 base62 characters.
    ///
    /// Examples: the parent identity of `docs/readme.txt` is the hash of `docs`;
    /// the parent identity of the directory `docs/notes` is the hash of `docs`.
    fn parent_identity(&self, path: &str) -> String;
}
