# UrlNormalize:

## Purpose

UrlNormalize turns a peer URL into its deterministic canonical identity, the
single form used everywhere a peer is compared to another peer or looked up in a
snapshot. It is a pure string-and-path transform: it reads only its input URL,
the current working directory, and the current OS user name, and it performs no
network, no connection, and no filesystem access. The same input URL always
produces the same canonical output (003.1 through 003.10), so two URLs that name
the same peer collapse to one identity and the rest of the program never has to
reason about scheme casing, default ports, slash noise, or query parameters.

Transport, the parent, calls UrlNormalize on each already-separated peer URL
before it selects a winning URL or opens a connection. UrlNormalize does not
recognize or split peer arguments on the command line and it does not connect to
anything; it consumes one URL string and returns one canonical URL string.

## Responsibilities

The operation UrlNormalize exposes across its boundary: given one URL string,
return its canonical form by applying every rule below. The rules combine into a
single deterministic transform.

- Lowercase the scheme (003.1) and lowercase the hostname (003.2). The rest of
  the URL keeps its original case.
- For an SFTP URL that names port 22, the default SFTP port, remove the port so
  the canonical form carries no explicit port (003.3).
- Collapse any run of consecutive slashes in the path to a single slash (003.4),
  and remove a trailing slash from the path (003.5). A path that reduces to the
  root stays a single slash rather than becoming empty.
- For a bare path with no scheme, convert it to a `file://` URL (003.6). For a
  `file://` URL, resolve its path to an absolute path from the current working
  directory (003.7), so a relative path becomes absolute against the process's
  working directory.
- Percent-decode unreserved characters in the URL (003.8): characters that do not
  need percent-encoding are returned in their plain form.
- Strip every query-string parameter, including the leading `?`, from the
  canonical form (003.9).
- For an SFTP URL that omits a username, insert the current OS user as the
  username (003.10). An SFTP URL that already names a username keeps it
  unchanged.

These worked examples are part of the observable behavior and must hold exactly:

- `c:/photos/` becomes `file:///c:/photos` (003.11).
- `./data`, from a working directory of `/home/user`, becomes
  `file:///home/user/data` (003.12).
- `SFTP://Host:22/path/` becomes `sftp://host/path` (003.13).
- `sftp://host//docs/` becomes `sftp://host/docs` (003.14).
- `sftp://host/path?timeout-conn=60` becomes `sftp://host/path` (003.15).
- `sftp://host/path`, run as OS user `ace`, becomes `sftp://ace@host/path`
  (003.16).

## Boundaries

Error obligations:

- UrlNormalize does no network, connection, or filesystem access, so it raises no
  transport, connection, or I/O errors. Its inputs are a URL string, the current
  working directory, and the current OS user name; it returns a canonical URL
  string.

Invariants:

- Normalization is deterministic: the same input URL, working directory, and OS
  user always produce the same canonical identity (003.1 through 003.10).
- All six worked examples hold exactly (003.11 through 003.16).
- The canonical form carries no default SFTP port, no consecutive or trailing
  slashes in the path, and no query string; its scheme and hostname are
  lowercase; an SFTP URL always names a username.

What UrlNormalize does not do:

- It does not recognize or split peer arguments on the command line; that parsing
  lives in `001_command-line`. It consumes one already-separated URL.
- It does not connect to a peer, authenticate, verify a host key, or select a
  winning URL among primary and fallback URLs; those belong to Transport and its
  connection backends.
- It does not percent-decode an inline SFTP password; password decoding before
  authentication is the SFTP backend's concern, not part of identity
  normalization.
- It holds no state and is a pure transform, so it depends on no sibling.
