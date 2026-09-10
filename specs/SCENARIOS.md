# Scenarios

These examples are part of the specification. Each scenario runs the released
executable for the current platform under `released/` (`kitchensync.exe`,
`kitchensync.mac`, or `kitchensync.linux`; see `DEVELOPMENT.md`). The scenarios
below write `released/kitchensync.exe` as shorthand for whichever one applies. Peer paths named `A`, `B`, and `C`
are directories under a test-created temporary directory.

Unless a scenario says otherwise, stdout and stderr are checked exactly, and
the listed file trees ignore `.kitchensync/` metadata directories. When a
scenario counts timestamp directories under `BAK/`, directories that contain
only an archived `manifest.txt` are not counted, and `manifest.txt` files
inside `BAK/` are never listed as user files.

## S-01: Help With No Arguments

Setup: no peer directories are needed.

Action: run `released/kitchensync.exe`. Then run it again with `--help`, with
`-h`, and with `/?`, each placed between two peer paths.

Outcome: every run exits 0. stdout is exactly the help text defined in
`help.md`, including its final newline. stderr is empty. The filesystem is not
changed. (An unrecognized argument also prints the help, after a one-line
error, but exits 1; see `help.md`.)

## S-02: First Sync From Canon

Setup:

- `A/album/one.txt` exists with bytes `canon\n` and modification time
  `2024-01-01_12-00-00_000000Z`.
- `B/` exists and has no user files.
- Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `B/album/one.txt` exists with bytes `canon\n` and modification time
`2024-01-01_12-00-00_000000Z`. Both peers contain
`.kitchensync/manifest.txt`, and so do the album directories:
`A/album/.kitchensync/manifest.txt` and `B/album/.kitchensync/manifest.txt`
exist.

## S-03: First Sync Without Canon Merges Both Ways

Setup:

- `A/readme.txt` exists with bytes `from A\n`.
- `B/other.txt` exists with bytes `from B\n`.
- Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly
`first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative\nsync complete\n`.
stderr is empty. `A/readme.txt` and `B/readme.txt` both contain `from A\n`, and
`A/other.txt` and `B/other.txt` both contain `from B\n`. Both peers contain
`.kitchensync/manifest.txt`.

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
exactly one timestamp-named directory holds user files, and it contains only
`old.txt` with bytes `remove me\n`. (Other timestamp directories may hold an
archived `manifest.txt`; those are not user files.)

## S-06: Subordinate Peer Receives The Group Outcome

Setup:

- `A/shared.txt` exists with bytes `group\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to
  exit 0.
- `C/shared.txt` exists with bytes `wrong\n`.
- `C/extra.txt` exists with bytes `extra\n`.
- `C/` has no `.kitchensync/manifest.txt`.

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
- Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity error +A B -x ignored`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `B/keep.txt` exists with bytes `copy\n`. `B/ignored/note.txt` still
exists with bytes `leave alone\n`. No `ignored/` entry is displaced to BAK on
either peer.

## S-08: Dry Run Does Not Change Peers

Setup:

- `A/dry.txt` exists with bytes `plan only\n`.
- `B/` exists and has no user files.
- Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --dry-run --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `dry run\nsync complete\n`.
stderr is empty. `B/` still has no user files. Neither peer has a
`.kitchensync/` directory at all.

## S-09: Canon File Replaces Directory Type Conflict

Setup:

- `A/item` is a file with bytes `file wins\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/item/nested.txt` exists with bytes `directory loses\n`.
- Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `B/item` is a file with bytes `file wins\n` and modification time
`2024-01-01_10-00-00_000000Z`. Under `B/.kitchensync/BAK/` there is exactly one
timestamp-named directory containing the displaced directory `item/` with
`nested.txt` inside it.

## S-10: New Peer Without A Manifest Is Subordinate

Setup:

- `A/shared.txt` exists with bytes `group\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to
  exit 0.
- `C/shared.txt` exists with bytes `wrong\n`.
- `C/extra.txt` exists with bytes `extra\n`.
- `C/` has no `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity error A B C`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. `C/shared.txt` exists with bytes `group\n` and modification time
`2024-01-01_10-00-00_000000Z`. `C/extra.txt` does not exist. The files under
`C/.kitchensync/BAK/*/` are exactly `shared.txt` with bytes `wrong\n` and
`extra.txt` with bytes `extra\n`. `C/.kitchensync/manifest.txt` exists.

## S-11: Info Verbosity Emits The Rollback Hint And Copy Progress

Setup:

- `A/note.txt` exists with bytes `copy me\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity info +A B`.

Outcome: the process exits 0. stdout is exactly three lines: the rollback hint,
then `C note.txt`, then `sync complete`. The test compares the first line by
pattern, not literally: it is
`undo later with: kitchensync --rollback <TS> <A> <B>`, where `<TS>` is any
27-character timestamp and `<A>` and `<B>` are the two peers' absolute paths as
KitchenSync displays them, with no `+` prefix. The remaining two lines are
compared literally. stderr is empty. `B/note.txt` exists with bytes `copy me\n`
and modification time `2024-01-01_10-00-00_000000Z`.

## S-12: Root Choice Does Not Matter

Setup:

- `A/b/c/one.txt` exists with bytes `one\n` and modification time
  `2024-01-01_10-00-00_000000Z`.
- `B/b/c/` exists and is empty.
- First run `released/kitchensync.exe --verbosity error +A/b/c B/b/c` and
  require it to exit 0.
- Create `A/b/two.txt` with bytes `two\n`.
- Delete `A/b/c/one.txt`.

Action: run `released/kitchensync.exe --verbosity error A/b B/b`.

Outcome: the process exits 0. stdout is exactly
`first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative\nsync complete\n`.
stderr is empty. `B/b/two.txt` exists with bytes `two\n`. `B/b/c/one.txt` does
not exist: the deletion under `c` was found in `c`'s own manifest, which the
earlier run wrote. Under `B/b/c/.kitchensync/BAK/` there is a timestamp-named
directory containing `one.txt` with bytes `one\n`. The notice on the first line
appears because the new sync root `b` has no manifest, even though `c` does.

## S-13: Undo Reverts A First-Sync Merge

Setup:

- `A/mine.txt` exists with bytes `mine\n`.
- `B/theirs.txt` exists with bytes `theirs\n`.
- Neither peer has `.kitchensync/manifest.txt`.
- First run `released/kitchensync.exe --verbosity error A B` and require it to
  exit 0. Both peers now have both files.

Action: run `released/kitchensync.exe --verbosity error --undo A B`.

Outcome: the process exits 0. stdout is exactly `rollback complete\n`. stderr is
empty. The user files under `A/` are exactly `mine.txt` with bytes `mine\n`, and
the user files under `B/` are exactly `theirs.txt` with bytes `theirs\n`.
`A/.kitchensync/BAK/` contains `theirs.txt` under one of its timestamp-named
directories, and `B/.kitchensync/BAK/` contains `mine.txt` the same way.

## S-14: Exclude Patterns, Ignore Files, And Negation

Setup:

- `A/.DS_Store`, `A/sub/.DS_Store`, and `A/sub/Thumbs.db` exist with bytes `x\n`.
- `A/keep.txt` exists with bytes `k\n`.
- `A/sub/note.tmp` exists with bytes `t\n` and `A/other.tmp` with bytes `t2\n`.
- `A/.kitchensync/ignore` contains the two lines `*.tmp` and `!other.tmp`.
- A local file `myignore` outside the peers contains the lines `# my file`
  and `!.DS_Store`.
- `B/` exists and has no user files. Neither peer has `.kitchensync/manifest.txt`.

Action: run `released/kitchensync.exe --verbosity error +A B -x @myignore -x sub/`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is
empty. The user files under `B/` are exactly `.DS_Store` with bytes `x\n`,
`keep.txt` with bytes `k\n`, and `other.tmp` with bytes `t2\n`. `B/sub` does
not exist. (`.DS_Store` is excluded by the built-ins but re-included by the
`@myignore` negation; `*.tmp` from A's ignore file is overridden for
`other.tmp` by that file's own negation; `sub/` is a directory-only pattern
from the command line.)

# Properties

## P-01: Output Channels

All KitchenSync output goes to stdout. stderr is empty for help, validation
errors, successful syncs, and recoverable sync diagnostics.

## P-02: Copy Limit

At no point may more than `--parallel` file transfers hold active copy slots
across the whole run, regardless of source scheme, destination scheme, peer, or
host.

## P-03: Peer Metadata Is Never Synced

`.kitchensync/` and `.git/` entries, symbolic links, and special files are not
part of the user file tree. They are omitted from listings, decisions, copies,
and manifest entries unless a spec section explicitly describes direct metadata
maintenance inside `.kitchensync/`.

## P-04: Manifest Replacement Never Renames Over A Live File

A directory's `.kitchensync/manifest.txt` is replaced only by the sequence in
manifest.md: write `manifest.txt.new`, move the live `manifest.txt` aside to
`manifest.txt.old`, rename `manifest.txt.new` into place, then move
`manifest.txt.old` to `.kitchensync/BAK/<timestamp>/manifest.txt`. No step ever
renames onto an existing path, so the run works on SFTP servers that reject
rename-over-existing, and a later normal run repairs any replacement that was
interrupted before it reads the directory.

## P-05: Dry Run Does Not Write Peer State

In `--dry-run`, KitchenSync connects, lists, reads manifests, decides, and
prints the same progress lines, but it must not create, modify, rename, delete,
or displace anything through a peer URL, and it writes no manifest. Source files
are not read and the copy queue is not exercised.
