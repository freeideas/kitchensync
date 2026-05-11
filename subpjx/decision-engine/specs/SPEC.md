# Per-entry sync decision logic — pure function over peer states and snapshot rows

## Purpose
Given the per-peer observations of one entry name at one directory level (each peer's listing state for that name) and the corresponding per-peer snapshot rows, decide what should happen at that path: which peers should receive a copy, which peers should have the entry displaced, which peers should create a directory, what the winning mod_time and byte_size are, and how the snapshot should be updated for each peer. The component implements entry classification, decision rules 1–6 (with 5-second timestamp tolerance), directory decisions, type-conflict resolution, and canon/subordinate handling. It performs no filesystem I/O, no networking, and no SQL; it is a deterministic function from inputs to a decision record.

## API surface

The component exposes a single operation:

**`decide(entry_name, per_peer_inputs) → decision`**

Inputs:
- `entry_name` — the basename of the entry being decided at the current directory level.
- `per_peer_inputs` — one record per peer that is active for this directory. Each record carries:
  - `peer_id` — opaque identifier the caller uses to correlate the decision back to its transport handle.
  - `role` — one of `contributing`, `canon`, or `subordinate`.
  - `listing_state` — one of: `live_file{mod_time, byte_size}`, `live_dir{mod_time}`, `absent` (the entry is not in the peer's listing of this directory).
  - `snapshot_row` — either `none` (no row exists for this peer/path) or a row carrying `mod_time`, `byte_size`, `last_seen` (nullable), and `deleted_time` (nullable).

Output: a `decision` record describing what happens at this path. Its shape conveys:
- `kind` — one of: `file`, `directory`, `type_conflict_file_wins`, `type_conflict_directory_wins`, `noop`.
- For `file` and `type_conflict_file_wins`:
  - `winning_mod_time`, `winning_byte_size`, `winning_source_peer_id` (the peer whose data should be read).
  - For each peer: an action — `copy_from_winner`, `already_matches` (no copy, just confirm), `displace_existing_file`, `displace_existing_directory`, `displace_then_copy`, or `no_action_no_row`.
- For `directory` and `type_conflict_directory_wins`:
  - For each peer: an action — `create_directory`, `displace_existing_file_then_create`, `displace_directory`, `recurse_only` (already a directory, keep), or `no_action_no_row`.
- For `noop`: every peer either already matches the group's view or has no row.
- Per-peer snapshot update directives co-located with each action (the caller persists these): `upsert_present{mod_time, byte_size, set_last_seen}`, `upsert_decided_target{mod_time, byte_size, last_seen_unchanged}`, `mark_tombstone{deleted_time}`, `clear_tombstone`, `no_change`. The "set_last_seen" flag distinguishes confirmation (listing observed live) from decided-but-not-yet-copied targets.

Auxiliary type used as input by both `decide` calls and by callers building snapshot rows from listings:

**`classify_file(listing_state, snapshot_row) → classification`**

Returns one of the values from the Entry Classification table: `unchanged`, `modified`, `resurrection`, `new`, `deleted{estimate}`, `absent_unconfirmed`, or `no_opinion`. Exposed because the caller may want to log or test classification independently; `decide` uses it internally.

Configuration the caller passes once at construction time:
- `timestamp_tolerance_seconds` — the tolerance window used in classification and decision-rule comparisons (the spec fixes this at 5; exposing it as a parameter keeps the component self-describing rather than hard-coding a magic number inside its API).
- `now` — the current sync run's timestamp, used as the value to assign to `last_seen` and `deleted_time` directives. Passing it in keeps the function pure (no clock read inside).

## Anchoring

- **Entry name, listing state, snapshot row, mod_time, byte_size, last_seen, deleted_time, parent_id** — `database.md` §"Schema", `multi-tree-sync.md` §"Entry Classification".
- **Classification values (unchanged, modified, resurrection, new, deleted, absent_unconfirmed)** — `multi-tree-sync.md` §"Entry Classification" table.
- **Decision rules 1–6, including tie-breaking on size and existence-over-deletion** — `multi-tree-sync.md` §"Decision Rules".
- **Rule 4b (absent-unconfirmed reconciliation against max peer mod_time)** — `multi-tree-sync.md` §"Decision Rules" rule 4b.
- **Directory decisions (existence-based, no mod_time)** — `multi-tree-sync.md` §"Directory Decisions".
- **Type-conflict resolution (file wins unless canon dictates otherwise)** — `multi-tree-sync.md` §"Type Conflicts".
- **Canon peer (`+`) override semantics and subordinate (`-`) non-voting semantics** — `sync.md` §"Canon Peer" and §"Subordinate Peer", `multi-tree-sync.md` §"Subordinate Peers".
- **5-second timestamp tolerance** — `multi-tree-sync.md` §"Decision Rules" (tolerance paragraph).
- **Snapshot update directives (upsert on confirm, decided-target without `last_seen`, tombstone on deletion, clear tombstone on resurrection)** — `multi-tree-sync.md` §"Snapshot Updates".
- **Determinism / pure-function shape** — well-known abstraction (pure function with no side effects), reinforced by `decomposition.md` §"decision-engine" ("pure function over peer states and snapshot rows, no filesystem, no networking, no SQL").
