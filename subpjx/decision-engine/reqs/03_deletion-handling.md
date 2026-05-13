# 03_deletion-handling: Deletion votes versus live observers

## Behavior
Tombstone history (`deleted_time` non-null) drives deletion votes. When some contributing participants are `Deleted` and others have live observations, the maximum `deleted_time` among the `Deleted` voters is compared to the newest live `mod_time` to decide whether deletion or survival wins. An `AbsentUnconfirmed` participant whose `last_seen` is recent enough triggers deletion; otherwise it receives the surviving entry. Derived from SPEC.md "Decision rules" rules 4 and 4b.

## $REQ_IDs
- `03.16` — When some contributing participants are `Deleted` and others have live observations, if the maximum `deleted_time` among the `Deleted` voters exceeds the maximum live `mod_time` by more than the tolerance, every participant holding the entry gets `Displace` and `entry_kind` is `None`.
- `03.17` — When some contributing participants are `Deleted` and others have live observations, if the maximum `deleted_time` among the `Deleted` voters does not exceed the maximum live `mod_time` by more than the tolerance, the entry survives and the live observer with the newest `mod_time` provides the winning metadata.
- `03.18` — When an `AbsentUnconfirmed` participant's `last_seen` is non-null and exceeds the maximum live `mod_time` by more than the tolerance, every participant holding the entry gets `Displace` and `entry_kind` is `None`.
- `03.19` — When the entry survives and an `AbsentUnconfirmed` participant's `last_seen` is null or does not exceed the maximum live `mod_time` by more than the tolerance, that participant receives the surviving entry (`ReceiveFile { source = winning_source }` or `CreateDirectory`).
- `03.20` — When no contributing participant has a live observation, `entry_kind` is `None`.
