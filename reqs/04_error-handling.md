# 04_error-handling: Runtime error semantics

## Behavior

Runtime errors during sync are handled per-operation: transfer/displacement failures log and skip, `set_mod_time` failures log a warning, snapshot upload failures log and leave staging behind. Sync continues when individual operations fail; correctness is restored on the next run. All transports surface a common set of error categories. Derived from `specs/sync.md` §"Errors" and §"Peer Transports" §"Error Semantics".

## $REQ_IDs
- `04.6` — A transfer failure is logged at `error` level and the file is skipped (no copy at the destination); it is re-discovered and re-attempted on the next run.
- `04.7` — A displacement failure (cannot rename to BAK/) is logged at `error` level and the displacement is skipped (the entry remains in place). If the displacement was part of a file copy sequence, the copy is also skipped and its TMP staging is cleaned up.
- `04.8` — A TMP staging failure (cannot create staging directory or write staging file) is treated as a transfer failure.
- `04.9` — A `set_mod_time` failure after a completed copy logs a warning and does not undo the copy.
- `04.10` — A snapshot upload failure is logged at `error` level; the TMP staging file is left in place to be cleaned up by `--xd` retention.
- `04.11` — All transports (`file://`, `sftp://`) surface the same error categories — not-found, permission denied, I/O error — to sync logic; transport-specific errors are not exposed.
