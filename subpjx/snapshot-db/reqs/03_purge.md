# 03_purge: Removing rows older than a retention window

## Behavior
`purge_older_than(handle, retention_days, now)` deletes rows in two classes: tombstones whose `deleted_time` is more than `retention_days` calendar days before `now`, and non-tombstone rows whose `last_seen` is either more than `retention_days` days before `now` or null. Rows whose relevant timestamp falls within the retention window are retained. Derived from `./specs/SPEC.md` § "Purge".

## $REQ_IDs
- `03.1` — `purge_older_than` deletes tombstone rows (`deleted_time` non-null) whose `deleted_time` is older than `retention_days` calendar days before `now`.
- `03.2` — `purge_older_than` deletes non-tombstone rows whose `last_seen` is older than `retention_days` calendar days before `now`.
- `03.3` — `purge_older_than` deletes non-tombstone rows whose `last_seen` is null.
- `03.4` — `purge_older_than` retains tombstone rows whose `deleted_time` is within `retention_days` of `now`.
- `03.5` — `purge_older_than` retains non-tombstone rows whose `last_seen` is within `retention_days` of `now`.
