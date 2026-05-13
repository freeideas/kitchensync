# Per-entry authoritative-state decision logic for N-participant tree reconciliation

## Purpose
Given a single named entry observed across N participants who each hold a copy of a shared tree, this library decides the entry's authoritative state and returns a per-participant action that brings every participant into agreement. Inputs are the participants' current observations of the entry plus each participant's prior recorded history of that entry; outputs are decision metadata and per-participant actions. The library is a pure function — no filesystem, no networking, no I/O, no storage.

## API surface

The library exposes one primary operation, `decide_entry`, operating on the data for one entry at a time. Callers invoke it once per entry encountered during their own tree walk; the library has no concept of recursion, of paths, or of trees of entries — only of the inputs and outputs described below for a single entry.

### Inputs

`decide_entry` takes four collections, all keyed by an opaque **participant identifier** the caller supplies:

1. **Roles.** Each participant has one role:
   - `canon` — at most one participant may have this role; its observation wins unconditionally.
   - `contributing` — participates in voting.
   - `subordinate` — does not vote; receives the outcome.

2. **Observations.** Each participant has exactly one current observation of the entry:
   - `File { mod_time, byte_size }` — a regular file, with its modification time and size in bytes.
   - `Directory` — a directory exists at this name.
   - `Absent` — no entry exists at this name on this participant.

3. **History records.** Each participant may have a prior recorded view of the entry, or none. A history record contains:
   - `mod_time` — the modification time observed last time.
   - `byte_size` — size in bytes for files, or the sentinel `-1` for directories.
   - `last_seen` — timestamp at which the entry was last confirmed present, or null (a write was decided but not yet confirmed).
   - `deleted_time` — timestamp at which the entry was confirmed absent, or null. A record with `deleted_time` non-null is a **tombstone**.

4. **Tolerance.** A non-negative duration (default: 5 seconds). All `mod_time` and `last_seen` comparisons treat two values as equal when they differ by at most this duration.

### Output

`decide_entry` returns a decision:

- `entry_kind` — one of `File`, `Directory`, or `None` (no entry exists in the group's view).
- `winning_mod_time`, `winning_byte_size` — when `entry_kind` is `File`, the metadata that should be recorded for the winning entry. For `Directory`, `winning_mod_time` is informational (directories are decided by existence, not by time); `winning_byte_size` is the directory sentinel `-1`. Absent when `entry_kind` is `None`.
- `winning_source` — the participant identifier whose observation provided the winning file metadata, or absent when `entry_kind` is `Directory` or `None`.
- For each participant identifier, an **action**:
   - `NoOp` — already matches the decided state.
   - `ReceiveFile { source }` — must obtain the file's content from `source` (a participant identifier).
   - `CreateDirectory` — must create a directory at this name.
   - `Displace` — currently holds data at this name that must be removed. The library does not prescribe where the removed data goes; that is the caller's concern.
- For each participant identifier, a **classification** of its prior state, for diagnostics: `Unchanged`, `Modified`, `Resurrected`, `New`, `Deleted`, `AbsentUnconfirmed`, or `NoOpinion`.

### Classification

For every participant, classify the entry by comparing observation against history:

| Observation                | History | `deleted_time` | Classification                          |
| -------------------------- | ------- | -------------- | --------------------------------------- |
| File, mod_time matches     | yes     | null           | `Unchanged`                             |
| File, mod_time differs     | yes     | null           | `Modified`                              |
| File or Directory          | yes     | not null       | `Resurrected`                           |
| File or Directory          | none    | —              | `New`                                   |
| Absent                     | yes     | not null       | `Deleted`                               |
| Absent                     | yes     | null           | `AbsentUnconfirmed`                     |
| Absent                     | none    | —              | `NoOpinion`                             |

`mod_time matches` is checked under the tolerance window.

### Decision rules

**With a canon participant.** The canon participant's observation wins unconditionally. If canon observes a `File`, every other participant that does not already match (within tolerance, same byte_size) gets `ReceiveFile { source = canon }`; canon itself gets `NoOp`. If canon observes a `Directory`, every other participant lacking it gets `CreateDirectory`; any with a file at this name (type conflict) gets `Displace` first (the directory-creation action subsumes that conflict's resolution). If canon observes `Absent`, every other participant that holds the entry gets `Displace`.

**Without a canon participant**, only contributing participants vote:

1. **All `Unchanged`** → every action is `NoOp`.
2. **`Modified`** → among contributing participants with a live observation, the newest `mod_time` wins. Tolerance applies: any live observer whose `mod_time` is within tolerance of the maximum is treated as tied with it. Participants whose observation does not match the winner get the appropriate corrective action.
3. **`New`** → as for `Modified`, the newest `mod_time` among live observers wins; participants lacking the entry receive it.
4. **`Deleted` + still-live observers** → compute the **deletion estimate** as the maximum `deleted_time` among `Deleted` voters. Compute the **max live mod_time** as the maximum `mod_time` among contributing live observers. If the deletion estimate exceeds the max live mod_time by more than the tolerance, deletion wins: all participants holding the entry get `Displace`. Otherwise the entry survives: rules 2/3/5/6 select the winner among live observers and propagate.
4b. **`AbsentUnconfirmed`** → for each such participant, compare its `last_seen` against the max live mod_time among contributing observers. If `last_seen` is non-null and exceeds the max live mod_time by more than the tolerance, this participant's classification is upgraded to a deletion vote with deletion estimate = `last_seen`, and rule 4 is applied. Otherwise treat as `NoOpinion` (an earlier copy was decided but never confirmed; the surviving entry simply propagates to this participant on the current decision).
5. **Tied `mod_time`, different `byte_size`** → larger `byte_size` wins.
6. **Other ties** → keep data: existence beats deletion, larger beats smaller.

Participants classified `NoOpinion` do not vote but become recipients (`ReceiveFile` or `CreateDirectory`) when the group decides the entry exists.

If no contributing participant votes for the entry's existence (every contributing participant is either `NoOpinion` or has been resolved as a deletion vote with no live counterpart), `entry_kind` is `None`; subordinate participants holding the entry get `Displace`.

After the winner is chosen, every participant — contributing or subordinate — is reconciled: participants whose observation already matches the winning state (file: same byte_size and `mod_time` within tolerance; directory: a directory exists) get `NoOp`; participants lacking the entry get `ReceiveFile { source = winning_source }` or `CreateDirectory`; participants holding data that does not match the decided kind or content get `Displace`.

### Type conflicts

When some participants observe a `File` and others observe a `Directory` for the same entry:

- If a canon participant has an observation, its type wins.
- Otherwise, the **file type wins**: every directory observer gets `Displace`, and the winning file is selected among the file observers by rules 1–6 above.

### Directory decisions

Directory entries are resolved by existence, not by `mod_time`:

- If any contributing participant currently observes a `Directory`, `entry_kind` is `Directory`. Participants lacking it get `CreateDirectory`; participants observing a `File` at this name get `Displace` (type conflict).
- If every contributing participant that has a history row for this name has a tombstone (`deleted_time` non-null) and currently observes `Absent`, the directory is collectively deleted: every participant still observing `Directory` or `File` here gets `Displace`, and `entry_kind` is `None`. A contributing participant with no history row at all is `NoOpinion` and does not block deletion.
- Otherwise, when no contributing participant observes or remembers the directory, `entry_kind` is `None`; subordinate participants holding anything at this name get `Displace`.

Canon overrides directory decisions in the same way as file decisions.

## Anchoring

- **Participant**, **observation** (`File`/`Directory`/`Absent`), **history record**, **role** (`canon`/`contributing`/`subordinate`), **action** (`NoOp`/`ReceiveFile`/`CreateDirectory`/`Displace`), **classification** (`Unchanged`/`Modified`/`Resurrected`/`New`/`Deleted`/`AbsentUnconfirmed`/`NoOpinion`), the decision-rule numbering 1–6, **type conflicts**, **directory decisions**, **tolerance**, and **deletion estimate** are introduced and defined by this library spec above. Downstream code refers to them by these names.
- `mod_time`, `byte_size`, `last_seen`, `deleted_time` — common file-metadata vocabulary (modification time, size in bytes, observation timestamps). The library does not constrain their concrete representation; it requires only an orderable timestamp type and an integer byte count.
- **Tombstone** — the standard term for a record marking a deletion.
- The library is a pure function with no I/O. No external protocol or system-level standard is needed beyond ordinary equality, ordering, and arithmetic on the input value types.
