# 003_peer-arguments: Peer argument syntax and role markers

## Behavior
This concern derives from `specs/sync.md` sections "Peers", "Fallback URLs",
"Per-URL Settings", "Command-Line Excludes", "Global Options", and "URL
Schemes", plus `specs/README.md` sections "First Sync", "Add A Peer",
"Fallback Paths", and "Exclude A Path". It covers the observable command-line
forms for peers, local paths, SFTP URLs, `+` and `-` prefixes, fallback URL
groups, per-URL settings, global options, and repeated `-x` arguments.

## $REQ_IDs
- `003.1` -- A sync invocation accepts a peer argument that is a local path.
- `003.2` -- A sync invocation accepts a peer argument that is an SFTP URL.
- `003.3` -- A bare peer path with no URL scheme is parsed as a local file peer.
- `003.4` -- A peer argument with a leading `+` is parsed as the canon peer.
- `003.5` -- A peer argument with a leading `-` is parsed as a subordinate peer.
- `003.6` -- A peer argument with no leading `+` or `-` is parsed as a normal bidirectional peer.
- `003.7` -- A sync invocation accepts no more than one peer argument with a leading `+`.
- `003.8` -- A sync invocation accepts multiple peer arguments with a leading `-`.
- `003.9` -- A bracketed comma-separated peer argument is parsed as one peer with multiple fallback URLs.
- `003.10` -- The fallback URLs inside a bracketed peer argument are parsed in the order they appear on the command line.
- `003.11` -- A leading `+` before a bracketed peer argument marks the whole fallback peer as canon.
- `003.12` -- A leading `-` before a bracketed peer argument marks the whole fallback peer as subordinate.
- `003.13` -- Role marker prefixes apply to the bracketed fallback peer, not to individual URLs inside the brackets.
- `003.14` -- A `timeout-conn` query parameter on a URL is parsed as the connection timeout setting for that URL.
- `003.15` -- A `timeout-idle` query parameter on a URL is parsed as the idle keep-alive setting for that URL.
- `003.16` -- A URL-level `timeout-conn` query parameter overrides the global `--timeout-conn` value for that URL.
- `003.17` -- A URL-level `timeout-idle` query parameter overrides the global `--timeout-idle` value for that URL.
- `003.18` -- A sync invocation rejects `max-copies` as a URL query parameter.
- `003.19` -- The `--dry-run` flag is parsed as read-only planning mode for the run.
- `003.20` -- Without `--dry-run`, read-only planning mode defaults to off.
- `003.21` -- The `--max-copies` option is parsed as the maximum number of concurrent copies across the whole run.
- `003.22` -- Without `--max-copies`, the maximum number of concurrent copies defaults to `10`.
- `003.23` -- The `--retries-copy` option is parsed as the number of copy tries before giving up.
- `003.24` -- Without `--retries-copy`, the number of copy tries before giving up defaults to `3`.
- `003.25` -- The `--retries-list` option is parsed as the number of listing tries before giving up.
- `003.26` -- Without `--retries-list`, the number of listing tries before giving up defaults to `3`.
- `003.27` -- The `--timeout-conn` option is parsed as the default SSH handshake timeout in seconds.
- `003.28` -- Without `--timeout-conn`, the default SSH handshake timeout is `30` seconds.
- `003.29` -- The `--timeout-idle` option is parsed as the default SFTP idle keep-alive time in seconds.
- `003.30` -- Without `--timeout-idle`, the default SFTP idle keep-alive time is `30` seconds.
- `003.31` -- The `--verbosity` option is parsed as the run's verbosity level.
- `003.32` -- Without `--verbosity`, the verbosity level defaults to `info`.
- `003.33` -- The `--keep-tmp-days` option is parsed as the stale TMP staging deletion age in days.
- `003.34` -- Without `--keep-tmp-days`, the stale TMP staging deletion age defaults to `2` days.
- `003.35` -- The `--keep-bak-days` option is parsed as the displaced-file deletion age in days.
- `003.36` -- Without `--keep-bak-days`, the displaced-file deletion age defaults to `90` days.
- `003.37` -- The `--keep-del-days` option is parsed as the deletion-record retention age in days.
- `003.38` -- Without `--keep-del-days`, the deletion-record retention age defaults to `180` days.
- `003.39` -- A Unix-style absolute path peer is parsed as a local file peer.
- `003.40` -- A Windows drive path peer is parsed as a local file peer.
- `003.41` -- A relative path peer is parsed as a local file peer.
- `003.42` -- An SFTP URL in the form `sftp://user@host/path` is parsed as an SFTP peer with the named user.
- `003.43` -- An SFTP URL in the form `sftp://user@host/path` is parsed with SSH port `22`.
- `003.44` -- An SFTP URL in the form `sftp://user@host:port/path` is parsed with the named SSH port.
- `003.45` -- An SFTP URL in the form `sftp://host/path` is parsed with the current operating-system user.
- `003.46` -- An SFTP URL in the form `sftp://user:password@host/path` is parsed with the inline password.
- `003.47` -- Percent-encoded `@` and `:` characters in an SFTP password are parsed as password characters.
- `003.48` -- The path portion of an SFTP URL is parsed as an absolute path from the remote filesystem root.
- `003.49` -- Each `-x <relative-path>` argument adds one command-line exclude path for the run.
- `003.50` -- Repeating `-x <relative-path>` adds multiple command-line exclude paths for the same run.
- `003.51` -- A command-line exclude path is parsed as a slash-separated relative path.
- `003.52` -- A command-line exclude path rejects a leading `/`.
- `003.53` -- A command-line exclude path rejects a trailing `/`.
- `003.54` -- A command-line exclude path rejects a `\` separator.
- `003.55` -- A command-line exclude path rejects empty path segments.
- `003.56` -- A command-line exclude path rejects a `.` path segment.
- `003.57` -- A command-line exclude path rejects a `..` path segment.
- `003.58` -- A command-line exclude path rejects NUL characters.

## Notes
This file covers syntax and parsed intent. Connection attempts, URL identity,
role effects during decisions, and exclude effects during traversal belong to
their own categories.
