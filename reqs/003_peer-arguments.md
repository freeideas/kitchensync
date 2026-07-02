# 003_peer-arguments: Peer argument syntax and role markers

## Behavior
This concern derives from `specs/sync.md` sections "Peers", "Fallback URLs",
"Per-URL Settings", "Command-Line Excludes", "Global Options", and "URL
Schemes", plus `specs/README.md` sections "First Sync", "Add A Peer",
"Fallback Paths", and "Exclude A Path". It covers the observable command-line
forms for peers, local paths, SFTP URLs, `+` and `-` prefixes, fallback URL
groups, per-URL settings, global options, and repeated `-x` arguments.

## $REQ_IDs
- `003.1` -- A non-help KitchenSync invocation with fewer than two peer arguments fails validation.
- `003.2` -- A peer argument can identify a sync target with a local path.
- `003.3` -- A peer argument can identify a sync target with an SFTP URL.
- `003.4` -- A peer path with no URL scheme is treated as a `file://` URL.
- `003.5` -- A Unix-style absolute path peer is treated as a local path.
- `003.6` -- A Windows drive path peer is treated as a local path.
- `003.7` -- A relative path peer is treated as a local path.
- `003.8` -- An SFTP URL in the form `sftp://user@host/path` uses the named user.
- `003.9` -- An SFTP URL in the form `sftp://user@host/path` uses SSH port `22`.
- `003.10` -- An SFTP URL in the form `sftp://user@host:port/path` uses the named SSH port.
- `003.11` -- An SFTP URL in the form `sftp://host/path` uses the current operating-system user.
- `003.12` -- An SFTP URL in the form `sftp://user:password@host/path` uses the inline password.
- `003.13` -- Percent-encoded `@` and `:` characters in an SFTP password are treated as password characters.
- `003.14` -- The path portion of an SFTP URL identifies an absolute path from the remote filesystem root.
- `003.15` -- A peer argument with a leading `+` marks that peer as the canon peer.
- `003.16` -- A peer argument with a leading `-` marks that peer as a subordinate peer.
- `003.17` -- A peer argument with no leading `+` or `-` marks that peer as a normal bidirectional peer.
- `003.18` -- A non-help KitchenSync invocation with more than one `+` peer fails validation.
- `003.19` -- A non-help KitchenSync invocation accepts multiple `-` peers.
- `003.20` -- A bracketed comma-separated peer argument identifies one peer with multiple fallback locations.
- `003.21` -- A bracketed fallback peer accepts local paths as fallback locations.
- `003.22` -- A bracketed fallback peer accepts SFTP URLs as fallback locations.
- `003.23` -- The fallback locations inside a bracketed peer argument keep their command-line order.
- `003.24` -- A leading `+` before a bracketed peer argument marks the whole fallback peer as canon.
- `003.25` -- A leading `-` before a bracketed peer argument marks the whole fallback peer as subordinate.
- `003.26` -- The role marker for a bracketed fallback peer is determined by the character before the opening bracket.
- `003.27` -- A `timeout-conn` query parameter on a URL supplies the connection timeout setting for that URL.
- `003.28` -- A `timeout-idle` query parameter on a URL supplies the idle keep-alive setting for that URL.
- `003.29` -- A URL-level `timeout-conn` query parameter overrides the global `--timeout-conn` value for that URL.
- `003.30` -- A URL-level `timeout-idle` query parameter overrides the global `--timeout-idle` value for that URL.
- `003.31` -- A non-help KitchenSync invocation rejects `max-copies` as a URL query parameter.
- `003.32` -- The `--dry-run` flag enables read-only planning mode for the run.
- `003.33` -- Without `--dry-run`, read-only planning mode is off.
- `003.34` -- The `--max-copies` option sets the maximum number of concurrent copies across the whole run.
- `003.35` -- Without `--max-copies`, the maximum number of concurrent copies is `10`.
- `003.36` -- The `--retries-copy` option sets the number of copy tries before giving up.
- `003.37` -- Without `--retries-copy`, the number of copy tries before giving up is `3`.
- `003.38` -- The `--retries-list` option sets the number of listing tries before giving up.
- `003.39` -- Without `--retries-list`, the number of listing tries before giving up is `3`.
- `003.40` -- The `--timeout-conn` option sets the default SSH handshake timeout in seconds.
- `003.41` -- Without `--timeout-conn`, the default SSH handshake timeout is `30` seconds.
- `003.42` -- The `--timeout-idle` option sets the default SFTP idle keep-alive time in seconds.
- `003.43` -- Without `--timeout-idle`, the default SFTP idle keep-alive time is `30` seconds.
- `003.44` -- The `--verbosity` option sets the run's verbosity level.
- `003.45` -- Without `--verbosity`, the verbosity level is `info`.
- `003.46` -- The `--keep-tmp-days` option sets the stale TMP staging deletion age in days.
- `003.47` -- Without `--keep-tmp-days`, the stale TMP staging deletion age is `2` days.
- `003.48` -- The `--keep-bak-days` option sets the displaced-file deletion age in days.
- `003.49` -- Without `--keep-bak-days`, the displaced-file deletion age is `90` days.
- `003.50` -- The `--keep-del-days` option sets the deletion-record retention age in days.
- `003.51` -- Without `--keep-del-days`, the deletion-record retention age is `180` days.
- `003.52` -- Each `-x <relative-path>` argument adds one command-line exclude path for the run.
- `003.53` -- Repeating `-x <relative-path>` adds multiple command-line exclude paths for the same run.
- `003.54` -- A command-line exclude path is parsed as a slash-separated relative path.
- `003.55` -- A command-line exclude path rejects a leading `/`.
- `003.56` -- A command-line exclude path rejects a trailing `/`.
- `003.57` -- A command-line exclude path rejects a `\` separator.
- `003.58` -- A command-line exclude path rejects empty path segments.
- `003.59` -- A command-line exclude path rejects a `.` path segment.
- `003.60` -- A command-line exclude path rejects a `..` path segment.
- `003.61` -- A command-line exclude path rejects NUL characters.

## Notes
This file covers syntax and parsed intent. Validation output, connection
attempts, URL identity, role effects during decisions, dry-run effects, copy
limits, verbosity output, retention cleanup, and exclude effects during
traversal belong to their own categories.
