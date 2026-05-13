# 04_type-and-directory: Type conflicts and directory-by-existence decisions

## Behavior
Directory entries are resolved by existence, not by `mod_time`. Without a canon participant, when contributing participants observe both `File` and `Directory` at the same name, the file type wins and the winning file is selected from the file observers. Without a canon participant, collective directory deletion applies when contributing participants with history rows hold tombstones and currently observe `Absent`; contributing participants with no history row and current `Absent` observations do not block that deletion. When no contributing participant observes or remembers the directory, the entry is absent. Derived from SPEC.md sections "Type conflicts" and "Directory decisions".

## $REQ_IDs
- `04.1` — When no canon participant exists, at least one contributing participant currently observes `Directory`, and no contributing participant currently observes `File`, `entry_kind` is `Directory`.
- `04.2` — When contributing participants include both `File` and `Directory` observations and no canon participant exists, `entry_kind` is `File`.
- `04.3` — When a file-versus-directory type conflict has multiple contributing `File` observers and no canon participant exists, the winning file is selected from the `File` observers using the no-canon voting rules.
- `04.4` — When no canon participant exists and every contributing participant currently observes `Absent` and has a tombstone history (`deleted_time` non-null), `entry_kind` is `None`.
- `04.5` — When no canon participant exists, every contributing participant currently observes `Absent`, at least one contributing participant has a tombstone history (`deleted_time` non-null), at least one contributing participant has no history row, and every contributing participant with a history row has a tombstone history, `entry_kind` is `None`.
- `04.6` — When no canon participant exists, every contributing participant currently observes `Absent`, and no contributing participant has a history row for this name, `entry_kind` is `None`.
