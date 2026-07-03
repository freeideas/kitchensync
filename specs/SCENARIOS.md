# Scenarios

These examples are part of the specification. Each scenario runs the released
executable at `released/kitchensync.exe`. Peer paths named `A`, `B`, and `C`
are directories under a test-created temporary directory.

Unless a scenario says otherwise, stdout and stderr are checked exactly, and
the listed file trees ignore `.kitchensync/` metadata directories.

## S-01: Help With No Arguments

Setup: no peer directories are needed.

Action: run `released/kitchensync.exe`.

Outcome: the process exits 0. stdout is exactly the help text defined in
`help.md`, including its final newline. stderr is empty. The filesystem is not
changed.

## S-02: First Sync From Canon

Setup:

- `A/album/one.txt` exists with bytes `canon\n` and modification time
  `2024-01-01_12-00-00_000000Z`.
- `B/` exists and has no user files.
- Neither peer has `.kitchensync/snapshot.db`.

Action: run `released/kitchensync.exe --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `B/album/one.txt` exists with bytes `canon\n` and modification time
`2024-01-01_12-00-00_000000Z`. Both peers contain
`.kitchensync/snapshot.db`.

## S-03: First Sync Without Canon Is Rejected

Setup:

- `A/readme.txt` exists with bytes `from A\n`.
- `B/` exists and has no user files.
- Neither peer has `.kitchensync/snapshot.db`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 1. stdout is exactly
`First sync? Mark the authoritative peer with a leading +\n`. stderr is empty.
`B/` still has no user files, and neither peer has `.kitchensync/snapshot.db`.

## S-04: Bidirectional Sync Chooses Newer Modification Time

Setup:

- `A/report.txt` exists with bytes `old\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to
  exit 0.
- Replace `B/report.txt` with bytes `new\n` and modification time
  `2024-01-02_10-00-00_000000Z`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `A/report.txt` and `B/report.txt` both contain bytes `new\n` and both
have modification time `2024-01-02_10-00-00_000000Z`.

## S-05: Deleted File Displaces Remaining Copies

Setup:

- `A/old.txt` exists with bytes `remove me\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to
  exit 0.
- Delete `A/old.txt`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `A/old.txt` and `B/old.txt` do not exist. Under `B/.kitchensync/BAK/`
there is exactly one timestamp-named directory, and it contains `old.txt` with
bytes `remove me\n`.

## S-06: Subordinate Peer Receives The Group Outcome

Setup:

- `A/shared.txt` exists with bytes `group\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to
  exit 0.
- `C/shared.txt` exists with bytes `wrong\n`.
- `C/extra.txt` exists with bytes `extra\n`.
- `C/` has no `.kitchensync/snapshot.db`.

Action: run `released/kitchensync.exe --verbosity error A B -C`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `C/shared.txt` exists with bytes `group\n` and modification time
`2024-01-01_10-00-00_000000Z`. `C/extra.txt` does not exist. The files under
`C/.kitchensync/BAK/*/` are exactly `shared.txt` with bytes `wrong\n` and
`extra.txt` with bytes `extra\n`.

## S-07: Command-Line Exclude Leaves Paths Untouched

Setup:

- `A/keep.txt` exists with bytes `copy\n`.
- `A/ignored/note.txt` exists with bytes `do not copy\n`.
- `B/ignored/note.txt` exists with bytes `leave alone\n`.
- Neither peer has `.kitchensync/snapshot.db`.

Action: run `released/kitchensync.exe --verbosity error +A B -x ignored`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `B/keep.txt` exists with bytes `copy\n`. `B/ignored/note.txt` still
exists with bytes `leave alone\n`. No `ignored/` entry is displaced to BAK on
either peer.

## S-08: Dry Run Does Not Change Peers

Setup:

- `A/dry.txt` exists with bytes `plan only\n`.
- `B/` exists and has no user files.
- Neither peer has `.kitchensync/snapshot.db`.

Action: run `released/kitchensync.exe --dry-run --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `dry run\nsync complete\n`.
stderr is empty. `B/` still has no user files. Neither peer has
`.kitchensync/snapshot.db`, `.kitchensync/TMP/`, `.kitchensync/SWAP/`, or
`.kitchensync/BAK/`.

## S-09: Canon File Replaces Directory Type Conflict

Setup:

- `A/item` is a file with bytes `file wins\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/item/nested.txt` exists with bytes `directory loses\n`.
- Neither peer has `.kitchensync/snapshot.db`.

Action: run `released/kitchensync.exe --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `B/item` is a file with bytes `file wins\n` and modification time
`2024-01-01_10-00-00_000000Z`. Under `B/.kitchensync/BAK/` there is exactly one
timestamp-named directory containing the displaced directory `item/` with
`nested.txt` inside it.

# Properties

## P-01: Output Channels

All KitchenSync output goes to stdout. stderr is empty for help, validation
errors, successful syncs, and recoverable sync diagnostics.

## P-02: Copy Limit

At no point may more than `--max-copies` file transfers hold active copy slots
across the whole run, regardless of source scheme, destination scheme, peer, or
host.

## P-03: Peer Metadata Is Never Synced

`.kitchensync/` and `.git/` entries, symbolic links, and special files are not
part of the user file tree. They are omitted from listings, decisions, copies,
and snapshot updates unless a spec section explicitly describes direct metadata
maintenance inside `.kitchensync/`.

## P-04: Snapshot Upload Is Atomic Through SWAP

A peer's live `.kitchensync/snapshot.db` is replaced only through the
`.kitchensync/SWAP/snapshot.db/` `new` and `old` paths. A later normal run
repairs any incomplete snapshot swap before deciding whether that peer has
snapshot history.

## P-05: Dry Run Does Not Write Peer State

In `--dry-run`, KitchenSync may create and update local temporary snapshot
databases, but it must not create, modify, rename, delete, displace, or upload
anything through a peer URL.
