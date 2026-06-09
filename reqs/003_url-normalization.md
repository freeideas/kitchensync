# 003_url-normalization: URL identity normalization

## Behavior
This concern derives from `specs/database.md` section "URL Normalization".

It covers the deterministic transformation that turns a peer URL into its
canonical identity used for comparison and snapshot lookup: lowercasing scheme
and hostname, removing the default SFTP port (22), collapsing consecutive
slashes, removing trailing slashes, converting bare paths to `file://` and
resolving them to absolute paths from the current working directory,
percent-decoding unreserved characters, stripping query-string parameters, and
inserting the current OS user for SFTP URLs that omit a username. The worked
examples in that section are part of the observable behavior.

How URLs are recognized and split on the command line is `001_command-line`.
How a chosen URL is connected and how a scheme selects a transport are
`005_connection-establishment` and `022_transports`.

## $REQ_IDs

- `003.1` -- Normalizing a URL lowercases the scheme.
- `003.2` -- Normalizing a URL lowercases the hostname.
- `003.3` -- Normalizing an SFTP URL that names port 22 removes the port.
- `003.4` -- Normalizing a URL collapses consecutive slashes in the path to a single slash.
- `003.5` -- Normalizing a URL removes a trailing slash from the path.
- `003.6` -- Normalizing a bare path with no scheme converts it to a `file://` URL.
- `003.7` -- Normalizing a `file://` URL resolves its path to an absolute path from the current working directory.
- `003.8` -- Normalizing a URL percent-decodes unreserved characters.
- `003.9` -- Normalizing a URL strips query-string parameters.
- `003.10` -- Normalizing an SFTP URL with no username inserts the current OS user as the username.
- `003.11` -- Normalizing `c:/photos/` produces `file:///c:/photos`.
- `003.12` -- Normalizing `./data` from a working directory of `/home/user` produces `file:///home/user/data`.
- `003.13` -- Normalizing `SFTP://Host:22/path/` produces `sftp://host/path`.
- `003.14` -- Normalizing `sftp://host//docs/` produces `sftp://host/docs`.
- `003.15` -- Normalizing `sftp://host/path?timeout-conn=60` produces `sftp://host/path`.
- `003.16` -- Normalizing `sftp://host/path` while running as OS user `ace` produces `sftp://ace@host/path`.
