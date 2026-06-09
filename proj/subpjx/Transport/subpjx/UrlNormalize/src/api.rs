//! Public specification for the `UrlNormalize` subproject.
//!
//! `UrlNormalize` turns a peer URL into its deterministic canonical identity,
//! the single form used everywhere a peer is compared to another peer or looked
//! up in a snapshot. It is a pure string-and-path transform: it reads only its
//! input URL, the current working directory, and the current OS user name, and
//! it performs no network, no connection, and no filesystem access.

/// Canonicalizes a peer URL into the single identity used for comparison and
/// snapshot lookup.
///
/// This is a pure transform that holds no state and depends on no sibling. It
/// reads only the input URL, the current working directory, and the current OS
/// user name; it does no network, connection, or filesystem access and so
/// raises no transport, connection, or I/O errors.
pub trait UrlNormalize: Send + Sync {
    /// Returns the canonical form of one already-separated peer URL.
    ///
    /// The transform applies every rule below in combination, so that two URLs
    /// naming the same peer collapse to one identity:
    ///
    /// - The scheme is lowercased and the hostname is lowercased; the rest of
    ///   the URL keeps its original case.
    /// - For an SFTP URL that names port 22, the default SFTP port, the port is
    ///   removed so the canonical form carries no explicit port.
    /// - Any run of consecutive slashes in the path collapses to a single
    ///   slash, and a trailing slash is removed from the path; a path that
    ///   reduces to the root stays a single slash rather than becoming empty.
    /// - A bare path with no scheme becomes a `file://` URL, and a `file://`
    ///   URL has its path resolved to an absolute path against the current
    ///   working directory.
    /// - Unreserved percent-encoded characters are decoded to their plain form.
    /// - Every query-string parameter, including the leading `?`, is stripped.
    /// - For an SFTP URL that omits a username, the current OS user is inserted
    ///   as the username; an SFTP URL that already names a username keeps it
    ///   unchanged.
    ///
    /// Normalization is deterministic: the same input URL, working directory,
    /// and OS user always produce the same canonical output. The canonical form
    /// carries no default SFTP port, no consecutive or trailing slashes in the
    /// path, and no query string; its scheme and hostname are lowercase; and an
    /// SFTP URL always names a username.
    ///
    /// These worked examples hold exactly:
    ///
    /// - `c:/photos/` becomes `file:///c:/photos`.
    /// - `./data`, from a working directory of `/home/user`, becomes
    ///   `file:///home/user/data`.
    /// - `SFTP://Host:22/path/` becomes `sftp://host/path`.
    /// - `sftp://host//docs/` becomes `sftp://host/docs`.
    /// - `sftp://host/path?timeout-conn=60` becomes `sftp://host/path`.
    /// - `sftp://host/path`, run as OS user `ace`, becomes
    ///   `sftp://ace@host/path`.
    fn normalize(&self, url: &str) -> String;
}
