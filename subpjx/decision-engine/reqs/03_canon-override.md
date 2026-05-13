# 03_canon-override: Canon participant wins unconditionally

## Behavior
When a participant has the `canon` role, its observation determines the authoritative state regardless of votes or observations from other participants. At most one participant may hold this role. Canon overrides type conflicts and directory decisions in the same way it overrides file decisions. Derived from SPEC.md "Inputs" → "Roles", "Output", "Decision rules" → "With a canon participant", "Type conflicts", and "Directory decisions".

## $REQ_IDs
- `03.1` — When canon observes `File`, `entry_kind` is `File` regardless of other participants' observations.
- `03.2` — When canon observes `File`, `winning_source` is the canon participant.
- `03.3` — When canon observes `File`, every other participant whose observation does not already match (within tolerance, same `byte_size`) gets `ReceiveFile { source = canon }`.
- `03.4` — When canon observes `Directory`, `entry_kind` is `Directory` regardless of other participants' observations.
- `03.5` — When canon observes `Directory`, every other participant lacking the entry gets `CreateDirectory`.
- `03.6` — When canon observes `Directory`, every other participant observing `File` at this name gets `Displace`.
- `03.7` — When canon observes `Absent`, `entry_kind` is `None` regardless of other participants' observations.
- `03.8` — When canon observes `Absent`, every other participant holding the entry gets `Displace`.
