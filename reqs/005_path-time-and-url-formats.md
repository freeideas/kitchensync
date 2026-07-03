# 005_path-time-and-url-formats: Path, time, and URL formats

## Behavior
This concern derives from `specs/database.md` sections "URL Normalization",
"Path Hashing", and "Timestamps", `specs/sync.md` sections "Command-Line
Excludes", "TMP Staging", "SWAP Directory", and "BAK Directory", and
`specs/multi-tree-sync.md` sections "Decision Rules", "Directory Decisions",
"Snapshot Updates", and "BAK/TMP Cleanup During Traversal". It covers the
observable normalization of peer URLs, relative slash-path rules, snapshot row
ID and parent ID format, metadata basename encoding, UTC timestamp string
format, per-process monotonic generated timestamps, copied deletion timestamp
semantics, and the five-second comparison tolerance.

## $REQ_IDs
- `005.1` -- Peer URL identity lowercases the URL scheme and hostname before comparison or lookup.
- `005.2` -- Peer URL identity removes port `22` from SFTP URLs before comparison or lookup.
- `005.3` -- Peer URL identity collapses consecutive slashes in the URL path before comparison or lookup.
- `005.4` -- Peer URL identity removes a trailing slash from the URL path before comparison or lookup.
- `005.5` -- Peer URL identity converts a peer argument with no URL scheme to a `file://` URL before comparison or lookup.
- `005.6` -- Peer URL identity resolves `file://` URL paths to absolute paths from the current working directory before comparison or lookup.
- `005.7` -- Peer URL identity percent-decodes unreserved characters before comparison or lookup.
- `005.8` -- Peer URL identity ignores URL query-string parameters before comparison or lookup.
- `005.9` -- Peer URL identity inserts the current OS username into an SFTP URL that has no username before comparison or lookup.
- `005.10` -- Exclude path arguments and progress-output paths use slash-separated relative paths with no leading slash, trailing slash, backslash separator, empty segment, `.`, `..`, or NUL character.
- `005.11` -- Snapshot path hashing uses slash-separated relative paths with no leading slash and no trailing slash.
- `005.12` -- Snapshot path hashing uses the same relative path bytes for files and directories at the same path.
- `005.13` -- KitchenSync creates no snapshot row for the sync root directory itself.
- `005.14` -- Snapshot row `id` values are 11-character, zero-padded base62 encodings of xxHash64 seed 0 over the entry's full relative path bytes.
- `005.15` -- Snapshot row `parent_id` values are 11-character, zero-padded base62 encodings of xxHash64 seed 0 over the parent directory's relative path bytes.
- `005.16` -- Snapshot row `parent_id` values for root entries use the 11-character, zero-padded base62 xxHash64 seed 0 encoding of `/`.
- `005.17` -- Snapshot row base62 IDs use the alphabet `0-9`, `A-Z`, `a-z`.
- `005.18` -- Every timestamp string written to snapshot columns, BAK directory names, TMP directory names, and log output uses UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` format with six microsecond digits.
- `005.19` -- Every generated current timestamp for a `last_seen` write, BAK directory name, or TMP directory name is strictly greater than every generated current timestamp already returned in the same process.
- `005.20` -- A `deleted_time` written for a confirmed absence is copied from the row's existing `last_seen` value.
- `005.21` -- A repeated confirmed absence for a row whose `deleted_time` is already set leaves the existing `deleted_time` unchanged.
- `005.22` -- A `deleted_time` written after a displacement to BAK is copied from that peer's row's existing `last_seen` value.
- `005.23` -- A displacement cascade writes the displaced entry's copied deletion estimate to affected descendant rows on the same peer.
- `005.24` -- Cleanup age for `.kitchensync/BAK/<timestamp>/` directories is determined from the `<timestamp>` path component.
- `005.25` -- Cleanup age for `.kitchensync/TMP/<timestamp>/` directories is determined from the `<timestamp>` path component.
- `005.26` -- User-entry SWAP paths use one percent-encoded basename path segment under `.kitchensync/SWAP/` for the target basename.
- `005.27` -- Snapshot database SWAP paths use `.kitchensync/SWAP/snapshot.db/new` and `.kitchensync/SWAP/snapshot.db/old`.
- `005.28` -- Entry classification treats a file's current `mod_time` as the same as the snapshot row's `mod_time` when the difference is no more than five seconds in either direction.
- `005.29` -- Entry classification treats a file's current `mod_time` as different from the snapshot row's `mod_time` when the difference is more than five seconds.
- `005.30` -- Peer `mod_time` decision comparisons treat any entry within five seconds of the maximum `mod_time` as tied with the maximum.
- `005.31` -- Peer `mod_time` decision comparisons treat any entry more than five seconds behind the maximum `mod_time` as older than the maximum.
- `005.32` -- File deletion estimates win over existing file `mod_time` values only when the deletion estimate is more than five seconds newer than the file `mod_time`.
- `005.33` -- An absent-unconfirmed file counts as a deletion only when its `last_seen` exceeds the maximum live-file `mod_time` by more than five seconds.
- `005.34` -- Directory deletion estimates are newer than live-subtree file evidence only when they exceed the newest live file `mod_time` by more than five seconds.
- `005.35` -- Directory decision timestamp evidence ignores directory `mod_time` values.
- `005.36` -- A live directory subtree that contains no files contributes no timestamp survival evidence for directory deletion comparison.

## Notes
This category owns shared data formats and comparisons. The database schema
that stores these values belongs to `004_snapshot-database-lifecycle`, and the
decisions that consume them belong to `007_reconciliation-decisions`.
