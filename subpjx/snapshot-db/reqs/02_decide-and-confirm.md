# 02_decide-and-confirm: Pending-decision writes and presence confirmation

## Behavior
`record_decided(handle, path, mod_time, byte_size, is_dir)` records that a decision has been made about `path` but its presence has not yet been confirmed: it writes `mod_time`, `byte_size` (with `-1` overriding when `is_dir` is true), and `deleted_time = null`, leaving `last_seen` null on insert and unchanged on update. `confirm_present(handle, path, now)` later sets `last_seen = now` on an existing row, leaving every other field as it was; if no row exists at `path`, it does nothing. Derived from `./specs/SPEC.md` § "Record operations" and § "Record shape".

## $REQ_IDs
- `02.1` — `record_decided` inserts a new row with `last_seen` null when no row exists at `path`.
- `02.2` — A row inserted by `record_decided` has the supplied `mod_time` and `byte_size` (with `-1` substituted when `is_dir` is true) and `deleted_time` null.
- `02.3` — Calling `record_decided` on an existing row updates its `mod_time` and `byte_size` and leaves its `last_seen` unchanged.
- `02.4` — `confirm_present` sets `last_seen` to the supplied `now` on the existing row at `path`, leaving every other field unchanged.
- `02.5` — `confirm_present` is a no-op when no row exists at `path` — it neither inserts a row nor modifies any other row.
- `02.6` — Calling `record_decided` on a tombstoned row (non-null `deleted_time`) clears `deleted_time` back to null.
