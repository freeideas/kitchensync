# Manifest

KitchenSync remembers what each peer had, so that on the next run it can tell a file that is new on one peer from a file that was deleted on another. That memory is a small text file in every synced directory, next to the entries it describes:

```
<dir>/.kitchensync/manifest.txt
```

There is no central database and no state keyed to the sync root. A directory's manifest describes only that directory's direct children, so the same manifests serve whether the directory is the sync root or is reached from a parent. Syncing `/X/b/c` with `/Y/b/c` today and `/X/b` with `/Y/b` tomorrow reuses the history under `c` unchanged.

## Format

One line per entry. Fields are tab-separated, in this order:

| Field          | Meaning                                                                                                    |
| -------------- | ---------------------------------------------------------------------------------------------------------- |
| `name`         | The entry's basename, percent-encoded so that tab, newline, carriage return, and `%` cannot appear raw.      |
| `kind`         | `f` for a file, `d` for a directory.                                                                       |
| `mod_time`     | `YYYY-MM-DD_HH-mm-ss_ffffffZ`: the entry's modification time as last observed on this peer. Recorded for directories but not used in decisions. |
| `byte_size`    | Bytes for files, `-1` for directories.                                                                     |
| `last_seen`    | Timestamp when the entry was last confirmed present on this peer (via listing or a completed copy), or `-` when a copy was decided but has not completed. |
| `deleted_time` | `-` while the entry exists. A timestamp once the entry has been confirmed absent (a tombstone).             |
| `placed`       | Timestamp when KitchenSync itself put the current content there - a copy it completed, or a directory it created - or `-` when the content came from the user or is not known. |

Lines are sorted by `name` (byte order). The file ends with a newline. A line that does not parse is ignored and dropped on the next rewrite. Unknown extra fields after `placed` are ignored, so the format can grow. A line from an older manifest with only six fields is read as `placed` `-`.

`placed` is what lets a rollback tell KitchenSync's own additions from the user's. It is set to a freshly generated timestamp when a copy into this directory completes or a directory is created here. A later listing that finds the entry unchanged (same mod_time and byte_size as its line) keeps the existing `placed`; a listing that finds a different mod_time or byte_size clears it to `-`, because the user has changed the content since.

Example:

```
IMG_0001.jpg	f	2024-03-02_09-15-30_000000Z	4194304	2024-03-05_08-00-01_120394Z	-	2024-03-05_08-00-01_120394Z
notes.txt	f	2024-01-01_12-00-00_000000Z	812	2024-03-05_08-00-01_120395Z	2024-03-05_08-00-01_120395Z	-
raw	d	2024-02-20_18-30-00_000000Z	-1	2024-03-05_08-00-01_120396Z	-	-
```

## Which directories have manifests

Every directory KitchenSync has synced on a peer gets a `.kitchensync/manifest.txt`, including the sync root. The manifest lives inside `.kitchensync/`, beside `BAK/` and `SWAP/`, so the built-in exclude keeps it out of the user's file tree. A directory that has never been synced simply has none.

When a directory is displaced to `BAK/`, its `.kitchensync/` folder and every manifest below it move with it. Nothing has to be cascaded or cleaned up: the history of a removed subtree leaves with the subtree.

## Reading

At each directory level of the walk, for each active peer, KitchenSync lists the directory and reads its manifest (if any) together. Missing manifest means "no history for this directory on this peer": every live entry is New, and every absent entry has no opinion. A read error other than "not found" is treated as a listing failure for that directory (see multi-tree-sync.md).

## Writing

A directory's manifest is rewritten on a peer only after every entry in that directory has been decided and every copy into that directory on that peer has finished or failed. Each active peer keeps an outstanding-copy count per directory for this purpose. A rewrite includes:

- one line per entry confirmed present (listed, created, or copied), with `last_seen` from that confirmation and `placed` as described under "Format";
- one line per queued copy destination that has not completed, with `last_seen` `-` and `placed` `-`;
- tombstone lines for entries confirmed absent or displaced, with `deleted_time` set as in "Tombstones" below;
- existing tombstones carried forward until they expire.

Tombstones older than `--keep-del-days` (by `deleted_time`) are dropped at rewrite time. Lines for excluded paths are carried forward unchanged.

If nothing in the manifest would change, it is not rewritten. In `--dry-run` manifests are never written.

The file is replaced without ever renaming over an existing file, so it works on SFTP servers that reject that: write `<dir>/.kitchensync/manifest.txt.new` and close it; rename the live `manifest.txt` to `manifest.txt.old` if it exists; rename `manifest.txt.new` to `manifest.txt`; move `manifest.txt.old` to `<dir>/.kitchensync/BAK/<timestamp>/manifest.txt`. These three names live directly inside `.kitchensync/`, where only KitchenSync's own files exist, so they cannot collide with user entries.

The old manifest is archived rather than deleted, in a fresh `BAK/<timestamp>/` directory created the same way a displacement creates its own. Kept beside the files that were displaced at the same time, it records what the directory held before this run, which is what makes a rollback possible (see "Rollback" below).

Before a directory is listed in a normal run, an interrupted manifest replacement is repaired:

- `manifest.txt.old` exists and `manifest.txt` exists: delete `manifest.txt.new` if present, then move `.old` to `BAK/<timestamp>/manifest.txt`.
- `.old` exists, `.new` exists, `manifest.txt` missing: rename `.new` to `manifest.txt`, then move `.old` to `BAK/<timestamp>/manifest.txt`.
- `.old` exists, `.new` missing, `manifest.txt` missing: rename `.old` to `manifest.txt`.
- `.old` missing, `.new` exists, `manifest.txt` exists: delete `.new`.
- `.old` missing, `.new` exists, `manifest.txt` missing: rename `.new` to `manifest.txt`.

## Tombstones

When an entry is confirmed absent on a peer whose manifest line has `deleted_time` `-`, the line is kept and `deleted_time` is set to a deletion estimate: the line's `last_seen` if present, otherwise a freshly generated timestamp. The estimate says "the deletion happened sometime after this". If `deleted_time` is already set, repeated confirmation of absence leaves it unchanged.

Tombstones are dropped when older than `--keep-del-days` (default: 180).

## History

A peer "has history" for a directory when that directory's `.kitchensync/manifest.txt` exists on the peer, even if it has no lines. The walk uses history per directory (see multi-tree-sync.md, "Directories Without History"). The startup rule about first syncs looks only at the sync root's manifest.

## Timestamps

Format: `YYYY-MM-DD_HH-mm-ss_ffffffZ`, UTC, microsecond precision, lexicographic sort, filesystem-safe. Used everywhere a timestamp appears: manifest fields, `BAK/` directory names, and log output.

Monotonic within a process: add 1 microsecond on collision. Every call site that needs a new "now" (each `last_seen` confirmation, each `placed` stamp, each `BAK/` directory name, each generated deletion estimate) calls the generator afresh and gets a value strictly greater than every value it has previously returned in this process. Do not capture one "run timestamp" at startup and reuse it.

A `deleted_time` copied from an existing `last_seen` is not a generator call and need not be unique.

## Rollback

Every change KitchenSync makes leaves the old thing behind in a `BAK/<timestamp>/` directory, and every manifest it replaces is archived there too. Together those two facts let a peer be put back the way it was at any moment inside the `--keep-bak-days` window. sync.md ("Rollback") describes the `--rollback <timestamp>` and `--undo` options that do this.

For one directory and a target time T, working from the peer root down:

1. List the directory's live entries, and list every `BAK/<ts>/` directory in it whose `<ts>` is later than T.
2. **Restore what was taken away.** For each name that appears in one or more of those BAK directories (ignoring `manifest.txt`), the copy in the earliest of them is what the directory held at T. Move any live entry of that name to `BAK/` first, then move the archived copy back into place, and print `R <relpath>`.
3. **Remove what was added.** For each live name with no BAK copy later than T, look at its line in the live manifest. If its `placed` is later than T, KitchenSync put it there after T: displace it to `BAK/` and print `X<peer> <relpath>` (see concurrency.md, "Progress Output", for the peer digit). Otherwise it was the user's own content from before T, or its history is unknown, so leave it alone.
4. **Recurse** into every live directory, including ones just restored - a directory that was displaced after T can still have had changes made inside it before that, and its own `BAK/` history covers them.
5. **Put the manifest back.** If any `BAK/<ts>/manifest.txt` with `<ts>` later than T exists, the earliest one becomes the live manifest again (the manifest it replaces is archived in the usual way). Otherwise rewrite the live manifest: drop the lines of removed entries entirely rather than tombstoning them - after an undo the peer has no opinion about those entries, so a later sync merges them back rather than deleting them elsewhere - and give restored entries fresh present lines with `placed` `-`.

Because `placed` marks only what KitchenSync itself put in place, and every removal goes to `BAK/` like any other deletion, a rollback never throws away a user's own file and can itself be rolled back.

Two limits are worth saying plainly:

- A rollback can only undo what KitchenSync did or saw. A change a user made between runs, in a directory KitchenSync had not listed again since, was never recorded and cannot be taken back.
- It reaches back only as far as `BAK/` goes. Once entries older than `--keep-bak-days` are cleaned up, the moments they belonged to are gone with them.
