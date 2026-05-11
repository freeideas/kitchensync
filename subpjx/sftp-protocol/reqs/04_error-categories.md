# 04_error-categories: Transport operations expose only the categorized error set

## Behavior

Every operation on a `Connection` reports failures using only the three categorized outcomes from the spec's error contract — `not found`, `permission denied`, and `I/O error` — and never surfaces transport-specific (SSH/SFTP) error codes to the caller. Derived from `SPEC.md` §"Operations on a `Connection`" (first paragraph).

## $REQ_IDs
- `04.1` — Every transport operation reports failure as one of `not found`, `permission denied`, or `I/O error`; transport-specific (SSH or SFTP protocol) error codes are not surfaced to the caller.

## Notes

Specific per-operation occurrences of these categories (e.g., `stat` returning `not found` for missing/symlink paths, handshake/auth failure as `I/O error`, unknown-host rejection as `I/O error`) are pinned in the relevant feature files (`02_filesystem-read`, `02_connection-lifecycle`, `03_authentication`). This file asserts the closed-set contract itself.
