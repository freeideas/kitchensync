# State

KitchenSync remembers what each peer had, so that on the next run it can tell a file that is new on one peer from a file that was deleted on another, and it records every change it makes, so that a run can be undone. Both live in one folder at the sync root of each peer:

```
<root>/.kitchensync/
  state.txt              names the format of the state (one line)
  state.gz               what this peer held, one line per entry in the whole tree
  journal/<run>.txt      every change one run made on this peer, for rollback
  BAK/<run>/<relpath>    entries KitchenSync displaced (kept --keep-bak-days days)
  runs.txt               one line per run (see sync.md, "Run Log")
  ignore                 optional exclude patterns (see sync.md, "Excludes")
```

`<run>` is the run's start timestamp. Apart from short-lived SWAP staging during a copy (see sync.md, "SWAP Directory"), nothing is kept in the user's other folders. Each peer has its own state; peers never read each other's.

Keeping everything at the root means a walk reads only the user's own folders: an unchanged folder costs one listing per peer, and the filesystem holds one small set of files instead of a metadata folder beside every user folder. On a filesystem with large allocation units (exFAT on a big drive allocates 128 KiB or more per file and per folder), that difference is gigabytes.

## State file

The state is a text file, kept compressed with gzip as `state.gz` because it holds one line per entry and is read whole at the start of every run, often over a slow link (for 60,000 entries, about 10 MB of text becomes about 1 MB). `state.txt` holds only the text's first line, which names the format, so that any KitchenSync can tell what it is looking at (see "Format version"). The text has one line per entry. Fields are tab-separated, in this order:

| Field          | Meaning                                                                                  |
| -------------- | ---------------------------------------------------------------------------------------- |
| `path`         | The entry's path relative to the sync root, `/`-separated, each segment percent-encoded so that tab, newline, carriage return, and `%` cannot appear raw. |
| `kind`         | `f` for a file, `d` for a directory.                                                     |
| `mod_time`     | The entry's modification time as last observed on this peer. Recorded for directories but not used in decisions. |
| `byte_size`    | Bytes for files, `-1` for directories.                                                   |
| `last_seen`    | Timestamp when the entry was confirmed present on this peer (via listing or a completed copy), or `-` when a copy was decided but has not completed. It is not refreshed while the entry stays unchanged (see multi-tree-sync.md, "State Updates"), so it is the first confirmation of the entry's current state, never later than the most recent one. |
| `deleted_time` | `-` while the entry exists. A timestamp once the entry has been confirmed absent (a tombstone). |

The first line is `#`, a tab, the format version (see "Format version"), a tab, and the timestamp at which the state was written. Other lines starting with `#` are ignored, as is any line that does not parse; it is dropped on the next rewrite. Extra fields after `deleted_time` are ignored, so the format can grow. Entry lines are sorted by `path` (byte order) and the file ends with a newline.

Example:

```
#	2	2024-03-05_08-10-00_000000Z
IMG_0001.jpg	f	2024-03-02_09-15-30_000000Z	4194304	2024-03-05_08-00-01_120394Z	-
raw	d	2024-02-20_18-30-00_000000Z	-1	2024-03-05_08-00-01_120396Z	-
raw/notes.txt	f	2024-01-01_12-00-00_000000Z	812	2024-03-05_08-00-01_120395Z	2024-03-05_08-00-01_120395Z
```

The lines for a directory's direct children are that directory's history on this peer. A peer "has history" at the sync root when `state.txt` exists there. The example and every rule below describe the text; on disk it is in `state.gz`.

### Format version

The state and every journal declare the format they are written in, currently `2`. Format 1 kept the whole state uncompressed in `state.txt`; it is still read, and the end of the next normal run rewrites it in format 2 even when nothing else changed. A journal's format is unchanged from 1 to 2. A KitchenSync that finds a newer format than it reads does not guess: a sync treats that peer as unreachable, with an error line naming the format; a nested root with a newer format is left out of the run's history and not written; and a rollback skips that peer and counts a failure. A peer in an older layout is converted when a run first meets it, as the per-directory layout is (see "Per-directory manifests").

### Reading

Each peer's state is read once, at startup, and held in memory for the run. A missing file means the peer has no history anywhere in the tree. A read error other than "not found" makes the peer unreachable.

Names are compared in NFC (see sync.md, "Unicode Normalization"). A path in another form counts as its NFC form; when two lines collapse to one, the live line is kept over a tombstone, then the one with the later `last_seen`.

### Writing

A directory's lines are settled once every entry in it has been decided and every copy into it on that peer has finished or failed (see multi-tree-sync.md, "State Updates"). Settled lines replace that directory's lines in memory. The state is written:

- at the end of the run, and
- during the run, at most every 5 minutes, whenever settled lines have changed since the last write, so that a run that is stopped keeps most of what it learned.

Lines for directories the run did not reach (excluded, skipped after a listing failure, or not visited) are carried forward unchanged. When a directory stops existing on a peer (displaced, or confirmed absent), every line beneath it is dropped from that peer's state. Tombstones older than `--keep-del-days` (by `deleted_time`) are dropped when the file is written. If nothing would change, it is not written. In `--dry-run` it is never written.

Writing replaces `state.gz` first and then `state.txt`, each without ever renaming over an existing file, so it works on SFTP servers that reject that: write `<name>.new` and close it; rename the live file to `<name>.old` if it exists; rename `<name>.new` to `<name>`; delete `<name>.old`. At startup, before reading, an interrupted replacement of either file is repaired (shown for `state.txt`; `state.gz` is the same):

- `.old` exists and `state.txt` exists: delete `.new` if present, then delete `.old`.
- `.old` exists, `.new` exists, `state.txt` missing: rename `.new` to `state.txt`, then delete `.old`.
- `.old` exists, `.new` missing, `state.txt` missing: rename `.old` to `state.txt`.
- `.old` missing, `.new` exists, `state.txt` exists: delete `.new`.
- `.old` missing, `.new` exists, `state.txt` missing: rename `.new` to `state.txt`.

`runs.txt` is replaced the same way.

A run stopped between the two replacements leaves the new `state.gz` behind the old `state.txt`: a format-2 `state.txt` names the same format, so the new `state.gz` is read; a format-1 `state.txt` is read as it stands, which is older history and safe. If a run stops before its final write, decisions on the next run still rest on what the peers actually hold. Lines that were not written are older than the truth, and an older `last_seen` errs toward keeping a file rather than deleting it.

### Tombstones

When an entry is confirmed absent on a peer whose line has `deleted_time` `-`, the line is kept and `deleted_time` is set to a deletion estimate: the line's `last_seen` if present, otherwise a freshly generated timestamp. The estimate says "the deletion happened sometime after this". If `deleted_time` is already set, repeated confirmation of absence leaves it unchanged. Tombstones are dropped when older than `--keep-del-days` (default: 180).

### Syncing part of a tree

A sync can start at a folder inside a tree that is usually synced from higher up: syncing `/X/b/c` on a day when `/X/b` is the usual root. When the sync root has neither a state of its own nor a per-directory manifest, KitchenSync looks upward, one folder at a time, for the nearest ancestor whose `.kitchensync/` holds a state (repairing an interrupted replacement there first, as at the root; a dry run repairs nothing). If it finds one:

- the peer has history, and its history is that state's lines for the subtree, rebased to the sync root;
- the run writes its lines back into that ancestor's state, replacing the subtree's lines and keeping all others, and writes no state at the sync root;
- the run's journal, BAK and run log are the ancestor's, with paths relative to the ancestor (`BAK/<run>/c/<path>`, journal paths `c/<path>`), so a later run or rollback from either folder reads the same records;
- an empty `.kitchensync` folder left at the sync root by SWAP staging is removed, as below the root.

A rollback started at the sync root finds the same ancestor and undoes the journal lines that lie inside its subtree. Exclude patterns come only from the sync root's own `.kitchensync/ignore` and the command line, since an ancestor's patterns are written relative to the ancestor. If no ancestor has a state, the peer has no history.

### Nested sync roots

A folder that has a state of its own (written when it was synced as a root while no ancestor had one) is a nested sync root. When the walk lists a directory below the root and the listing shows a `.kitchensync` folder there holding `state.txt`, that directory is a nested sync root. If its state was written later than the outer root's, its lines replace the outer state's lines for that subtree on that peer (with the nested directory's path prefixed). At the end of the run, the outer run writes the nested root's state too, with the run's lines for that subtree, so both roots stay current. A nested root's `journal/`, `BAK/` and `runs.txt` belong to it and are left alone.

### Per-directory manifests

KitchenSync also reads the per-directory layout, where every synced directory held `.kitchensync/manifest.txt` describing its direct children (fields `name`, `kind`, `mod_time`, `byte_size`, `last_seen`, `deleted_time`, then fields that are ignored) and displaced entries sat in `<dir>/.kitchensync/BAK/<ts>/`. A peer whose root has `.kitchensync/manifest.txt` but no `state.txt` has history, and the root's manifest supplies the root's lines. When the walk lists a directory below the root whose `.kitchensync` folder holds `manifest.txt` (or `manifest.txt.new` or `manifest.txt.old`), and the peer's state has no lines for that directory's children, that manifest supplies them.

In a normal run the per-directory folder is then converted, right after it is read: every entry of `<dir>/.kitchensync/BAK/<ts>/` moves to `<root>/.kitchensync/BAK/<ts>/<dir>/`, except that a `BAK/<ts>/` holding only `manifest.txt` whose every line parses as a manifest line is an archived manifest and is deleted (the state file now carries that history), the manifest files are deleted, any SWAP staging is recovered, and the emptied folders are removed (operating-system litter, as defined in sync.md, "SWAP Directory", does not keep a folder in place). A conversion step that fails is reported at error level and retried on the next run; it never blocks the sync. The root's own `.kitchensync/BAK/<ts>/<name>` entries already have the root layout and stay where they are.

## Journal

Every normal run writes `<root>/.kitchensync/journal/<run>.txt` on each peer where it changes something; a run that changes nothing on a peer writes no journal there. Its first line is `#`, a tab, and the format version. Each change is appended as soon as it succeeds. One line per change, tab-separated:

```
<timestamp>  <op>  <path>  <other>  <byte_size>  <mod_time>
```

`<timestamp>` is a freshly generated timestamp. Paths are relative to the sync root and encoded as in the state. Fields that do not apply are `-`.

| Op  | Meaning                                                                                   |
| --- | ----------------------------------------------------------------------------------------- |
| `X` | The entry at `path` was moved to `other`, a path under `BAK/`. For a file, size and mod_time are given. |
| `B` | `path` is a file inside a directory just displaced by an `X` line, now in BAK at `other`, with its size and mod_time. Used only to find moved files (see sync.md, "Moved Files"). |
| `C` | KitchenSync wrote file content at `path`, with the given size and mod_time.               |
| `D` | KitchenSync created the directory `path`.                                                 |
| `M` | KitchenSync moved the file at `other` (a live path or a BAK path) to `path`, with the given size and mod_time. |

A replaced file appears as an `X` line for the old content followed by a `C` or `M` line for the new. A line that does not parse (for example one cut short when a run was stopped) is ignored. Journals older than `--keep-bak-days`, by their name, are deleted along with expired BAK folders (see "BAK").

## BAK

A displaced entry is renamed to `<root>/.kitchensync/BAK/<run>/<relpath>`, creating parent directories as needed. If that path is already taken in this run, a freshly generated timestamp is used instead of `<run>`. A displaced directory moves as one rename with its whole subtree. At the end of each normal sync, after the last copy, `BAK/<ts>/` folders older than `--keep-bak-days` days (by `<ts>`) are deleted. Doing it last keeps the cost of listing a large BAK out of the time before the first progress line.

## Rollback

The journals record everything KitchenSync did, so a peer can be put back the way it was at any moment inside the `--keep-bak-days` window. sync.md ("Rollback") describes the `--rollback <timestamp>` and `--undo` options that do this.

A rollback to target time T reads every journal on the peer, takes the lines whose timestamp is later than T, and undoes them one at a time, newest first:

- **`C path`**: if `path` is still a file with that size and a mod_time within 5 seconds, displace it to BAK and print `X<peer> <path>`. Otherwise the user has changed it since: leave it alone.
- **`D path`**: if `path` is still a directory, displace it to BAK and print `X<peer> <path>`.
- **`M path other`**: if `path` is still a file with that size and mod_time, move it back to `other` (creating parent directories). Print `R<peer> <other>` when `other` is not under `BAK/`; a file returned to BAK is printed when a later step restores it. If `other` is occupied, leave the file in place and count a failure.
- **`X path other`**: move `other` back to `path`, displacing anything now at `path` to BAK first, and print `R<peer> <path>`. If `other` no longer exists, count a failure.
- **`B` lines** are skipped.

Then the state is adjusted and written: lines for paths removed by the rollback, and every line beneath them, are dropped, so the peer has no opinion about those entries and a later sync merges them back rather than deleting them elsewhere; restored paths get fresh present lines.

A rollback writes its own journal, under its own start timestamp, so it can itself be rolled back. Every removal goes to BAK like any other displacement, so a rollback never throws away a user's file.

Two limits are worth saying plainly:

- A rollback can only undo what KitchenSync did. A change a user made between runs was never recorded and is not taken back.
- It reaches back only as far as the journals and BAK go. Once they expire after `--keep-bak-days`, the moments they belonged to are gone with them.

## Timestamps

Format: `YYYY-MM-DD_HH-mm-ss_ffffffZ`, UTC, microsecond precision, lexicographic sort, filesystem-safe. Used everywhere a timestamp appears: state and journal fields, `BAK/` and `journal/` names, and log output.

Monotonic within a process: add 1 microsecond on collision. Every call that needs a new "now" (each `last_seen` confirmation, each journal line, each generated deletion estimate) gets a value strictly greater than every value previously returned in this process. A `deleted_time` copied from an existing `last_seen` is not a generator call and need not be unique.
