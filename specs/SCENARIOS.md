# Scenarios

These examples are part of the specification. Each scenario runs the released executable for the current platform under `released/` (`kitchensync.exe`, `kitchensync.mac`, or `kitchensync.linux`; see `DEVELOPMENT.md`). The scenarios below write `released/kitchensync.exe` as shorthand for whichever one applies. Peer paths named `A`, `B`, and `C` are directories under a test-created temporary directory.

Unless a scenario says otherwise, stdout and stderr are checked exactly, and the listed file trees ignore `.kitchensync/` metadata directories. "BAK" means `<peer>/.kitchensync/BAK/`, and "the files in BAK" are the files beneath its timestamp-named directories, by their paths below those directories.

## S-01: Help With No Arguments

Setup: no peer directories are needed.

Action: run `released/kitchensync.exe`. Then run it again with `--help`, with `-h`, and with `/?`, each placed between two peer paths.

Outcome: every run exits 0. stdout is exactly the help text defined in `help.md`, including its final newline. stderr is empty. The filesystem is not changed. (An unrecognized argument also prints the help, after a one-line error, but exits 1; see `help.md`.)

## S-02: First Sync From Canon

Setup:

- `A/album/one.txt` exists with bytes `canon\n` and modification time `2024-01-01_12-00-00_000000Z`.
- `B/` exists and has no user files.
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `B/album/one.txt` exists with bytes `canon\n` and modification time `2024-01-01_12-00-00_000000Z`. Both peers contain `.kitchensync/state.txt`, and neither `A/album/.kitchensync` nor `B/album/.kitchensync` exists.

## S-03: First Sync Without Canon Merges Both Ways

Setup:

- `A/readme.txt` exists with bytes `from A\n`.
- `B/other.txt` exists with bytes `from B\n`.
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative\nsync complete\n`. stderr is empty. `A/readme.txt` and `B/readme.txt` both contain `from A\n`, and `A/other.txt` and `B/other.txt` both contain `from B\n`. Both peers contain `.kitchensync/state.txt`.

## S-04: Bidirectional Sync Chooses Newer Modification Time

Setup:

- `A/report.txt` exists with bytes `old\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- Replace `B/report.txt` with bytes `new\n` and modification time `2024-01-02_10-00-00_000000Z`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `A/report.txt` and `B/report.txt` both contain bytes `new\n` and both have modification time `2024-01-02_10-00-00_000000Z`.

## S-05: Deleted File Displaces Remaining Copies

Setup:

- `A/old.txt` exists with bytes `remove me\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- Delete `A/old.txt`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `A/old.txt` and `B/old.txt` do not exist. B's BAK holds exactly one timestamp-named directory, and it contains only `old.txt` with bytes `remove me\n`.

## S-06: Subordinate Peer Receives The Group Outcome

Setup:

- `A/shared.txt` exists with bytes `group\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- `C/shared.txt` exists with bytes `wrong\n`.
- `C/extra.txt` exists with bytes `extra\n`.
- `C/` has no `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error A B -C`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `C/shared.txt` exists with bytes `group\n` and modification time `2024-01-01_10-00-00_000000Z`. `C/extra.txt` does not exist. The files in C's BAK are exactly `shared.txt` with bytes `wrong\n` and `extra.txt` with bytes `extra\n`.

## S-07: Command-Line Exclude Leaves Paths Untouched

Setup:

- `A/keep.txt` exists with bytes `copy\n`.
- `A/ignored/note.txt` exists with bytes `do not copy\n`.
- `B/ignored/note.txt` exists with bytes `leave alone\n`.
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error +A B -x ignored`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `B/keep.txt` exists with bytes `copy\n`. `B/ignored/note.txt` still exists with bytes `leave alone\n`. No `ignored/` entry is displaced to BAK on either peer.

## S-08: Dry Run Does Not Change Peers

Setup:

- `A/dry.txt` exists with bytes `plan only\n`.
- `B/` exists and has no user files.
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --dry-run --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `dry run\nsync complete\n`. stderr is empty. `B/` still has no user files. Neither peer has a `.kitchensync/` directory at all.

## S-09: Canon File Replaces Directory Type Conflict

Setup:

- `A/item` is a file with bytes `file wins\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/item/nested.txt` exists with bytes `directory loses\n`.
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error +A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `B/item` is a file with bytes `file wins\n` and modification time `2024-01-01_10-00-00_000000Z`. B's BAK holds exactly one timestamp-named directory, containing the displaced directory `item/` with `nested.txt` inside it.

## S-10: New Peer Without History Is Subordinate

Setup:

- `A/shared.txt` exists with bytes `group\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- `C/shared.txt` exists with bytes `wrong\n`.
- `C/extra.txt` exists with bytes `extra\n`.
- `C/` has no `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error A B C`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `C/shared.txt` exists with bytes `group\n` and modification time `2024-01-01_10-00-00_000000Z`. `C/extra.txt` does not exist. The files in C's BAK are exactly `shared.txt` with bytes `wrong\n` and `extra.txt` with bytes `extra\n`. `C/.kitchensync/state.txt` exists.

## S-11: Info Verbosity Emits The Rollback Hint And Copy Progress

Setup:

- `A/note.txt` exists with bytes `copy me\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity info +A B`.

Outcome: the process exits 0. stdout is exactly three lines: the rollback hint, then `1C2 note.txt`, then `sync complete`. The test compares the first line by pattern, not literally: it is `undo later with: kitchensync --rollback <TS> <A> <B>`, where `<TS>` is any 27-character timestamp and `<A>` and `<B>` are the two peers' absolute paths as KitchenSync displays them, with no `+` prefix. The remaining two lines are compared literally. stderr is empty. `B/note.txt` exists with bytes `copy me\n` and modification time `2024-01-01_10-00-00_000000Z`.

## S-12: Root Choice Does Not Matter

Setup:

- `A/b/c/one.txt` exists with bytes `one\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/b/c/` exists and is empty.
- First run `released/kitchensync.exe --verbosity error +A/b/c B/b/c` and require it to exit 0.
- Create `A/b/two.txt` with bytes `two\n`.
- Delete `A/b/c/one.txt`.

Action: run `released/kitchensync.exe --verbosity error A/b B/b`.

Outcome: the process exits 0. stdout is exactly `first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative\nsync complete\n`. stderr is empty. `B/b/two.txt` exists with bytes `two\n`. `B/b/c/one.txt` does not exist: the deletion under `c` was found in `c`'s own state, which the earlier run wrote. The files in `B/b`'s BAK are exactly `c/one.txt` with bytes `one\n`. `B/b/c/.kitchensync/state.txt` no longer lists `one.txt` as live. The notice on the first line appears because the new sync root `b` has no state, even though `c` does.

## S-13: Undo Reverts A First-Sync Merge

Setup:

- `A/mine.txt` exists with bytes `mine\n`.
- `B/theirs.txt` exists with bytes `theirs\n`.
- Neither peer has a `.kitchensync/` directory.
- First run `released/kitchensync.exe --verbosity error A B` and require it to exit 0. Both peers now have both files.

Action: run `released/kitchensync.exe --verbosity error --undo A B`.

Outcome: the process exits 0. stdout is exactly `rollback complete\n`. stderr is empty. The user files under `A/` are exactly `mine.txt` with bytes `mine\n`, and the user files under `B/` are exactly `theirs.txt` with bytes `theirs\n`. The files in A's BAK are exactly `theirs.txt`, and in B's BAK exactly `mine.txt`.

## S-14: Exclude Patterns, Ignore Files, And Negation

Setup:

- `A/.DS_Store`, `A/sub/.DS_Store`, and `A/sub/Thumbs.db` exist with bytes `x\n`.
- `A/keep.txt` exists with bytes `k\n`.
- `A/sub/note.tmp` exists with bytes `t\n` and `A/other.tmp` with bytes `t2\n`.
- `A/.kitchensync/ignore` contains the two lines `*.tmp` and `!other.tmp`.
- A local file `myignore` outside the peers contains the lines `# my file` and `!.DS_Store`.
- `B/` exists and has no user files. Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error +A B -x @myignore -x sub/`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. The user files under `B/` are exactly `.DS_Store` with bytes `x\n`, `keep.txt` with bytes `k\n`, and `other.tmp` with bytes `t2\n`. `B/sub` does not exist. (`.DS_Store` is excluded by the built-ins but re-included by the `@myignore` negation; `*.tmp` from A's ignore file is overridden for `other.tmp` by that file's own negation; `sub/` is a directory-only pattern from the command line.)

## S-15: Folder Renamed On One Peer Is Not Copied Back

Setup:

- `A/old/movie.txt` exists with bytes `movie\n` and `A/old/thumbs/1.txt` with bytes `thumb\n`, both with modification time `2024-01-01_10-00-00_000000Z`.
- `B/` exists and has no user files.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- Rename `B/old` to `B/new`.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. The user files under both `A/` and `B/` are exactly `new/movie.txt` with bytes `movie\n` and `new/thumbs/1.txt` with bytes `thumb\n`. Neither `A/old` nor `B/old` exists. (B's deletion of `old` carries into the folder: everything inside is older than the deletion, so it is removed from A rather than copied back to B.)

## S-16: Names In Different Unicode Forms Are The Same File

Setup:

- `A/café.txt` exists with the name in composed form (NFC: `é` is the single character U+00E9), bytes `same\n`, and modification time `2024-01-01_10-00-00_000000Z`.
- `B/café.txt` exists with the name in decomposed form (NFD: `e` followed by the combining accent U+0301), the same bytes, and the same modification time. (macOS reports names this way.)
- Neither peer has a `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error A B` twice.

Outcome: both runs exit 0 with empty stderr. The first run's stdout is the first-sync line (see sync.md, "Startup") followed by `sync complete\n`; the second run's stdout is exactly `sync complete\n`. After each run the user files under `A/` are exactly the composed `café.txt` and under `B/` exactly the decomposed `café.txt`, each with bytes `same\n`. Neither peer has a BAK.

## S-17: An Interrupted Copy Is Not Put In Place

Setup:

- `A/keep.txt` exists with bytes `keep\n` and `A/movie.bin` with bytes `0123456789\n`, both with modification time `2024-01-01_10-00-00_000000Z`.
- `B/keep.txt` exists with bytes `keep\n` and the same modification time. `B/` has no `movie.bin`.
- First run `released/kitchensync.exe --verbosity error A B -x movie.bin` and require it to exit 0, so both peers have state and B has never had `movie.bin`.
- Simulate a copy of `movie.bin` to B that was cut short: create `B/.kitchensync/SWAP/movie.bin/new` with bytes `01234` and the current time as its modification time.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `A/movie.bin` and `B/movie.bin` both hold `0123456789\n` with modification time `2024-01-01_10-00-00_000000Z`. `B/.kitchensync/SWAP` does not exist.

## S-18: Mac Litter In SWAP Does Not Block Cleanup

Setup:

- `A/keep.txt` and `B/keep.txt` exist with bytes `keep\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `A/movie.bin` exists with bytes `new\n` and modification time `2024-01-02_10-00-00_000000Z`.
- `B/movie.bin` exists with bytes `old\n` and modification time `2024-01-01_10-00-00_000000Z`.
- First run `released/kitchensync.exe --verbosity error A B -x movie.bin` and require it to exit 0, so both peers have state.
- Simulate the files macOS leaves on an exFAT drive after touching an earlier swap: create `B/.kitchensync/SWAP/movie.bin/._new` and `B/.kitchensync/SWAP/._movie.bin`, each with 4096 zero bytes.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. `A/movie.bin` and `B/movie.bin` both hold `new\n`. `B/.kitchensync/SWAP` does not exist.

## S-19: A Moved File Is Moved, Not Copied Again

Setup:

- `A/movie.bin` exists with 2 MiB of content: the bytes 0 to 255 in order, repeated 8,192 times. Its modification time is `2024-01-01_10-00-00_000000Z`.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0, so `B/movie.bin` exists with the same content and modification time.
- On A, move `A/movie.bin` to `A/shows/film.bin` (a rename, which keeps its modification time).

Action: run `released/kitchensync.exe --verbosity info A B`.

Outcome: the process exits 0 with empty stderr. stdout is exactly four lines: the rollback hint, `X2 movie.bin`, `M2 shows/film.bin`, and `sync complete`. `B/shows/film.bin` holds the original 2 MiB content with modification time `2024-01-01_10-00-00_000000Z`, and B has no `movie.bin`. Nothing named `movie.bin` remains in B's BAK: the displaced file was moved into place rather than kept there.

## S-20: Same Size And Time But Different Content Is Copied

Setup: the same as S-19, except that after the move, `A/shows/film.bin` is rewritten with the same content changed in one byte at offset 1,048,576 (the middle of the file), and its modification time is set back to `2024-01-01_10-00-00_000000Z`.

Action: run `released/kitchensync.exe --verbosity info A B`.

Outcome: the process exits 0 with empty stderr. stdout is exactly four lines: the rollback hint, `X2 movie.bin`, `1C2 shows/film.bin`, and `sync complete`. `B/shows/film.bin` holds A's changed content. B's BAK holds `movie.bin` with the original content.

## S-21: Undo Puts A Moved File Back

Setup: the same as S-19, including its action (run with `--verbosity error` instead of `info`).

Action: run `released/kitchensync.exe --verbosity error --undo B`.

Outcome: the process exits 0. stdout is exactly `rollback complete\n`. stderr is empty. The user files under `B/` are exactly `movie.bin` with the original 2 MiB content and modification time `2024-01-01_10-00-00_000000Z`; there is no `B/shows/film.bin`.

## S-22: A Run Over An Unchanged Tree Writes No State

Setup:

- `A/top.txt` and `A/sub/inner.txt` exist with bytes `x\n`, both with modification time `2024-01-01_10-00-00_000000Z`. `B/` exists and is empty.
- Run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- Record the bytes of every file under each peer's `.kitchensync/` directory.

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n`. stderr is empty. On each peer, every file under `.kitchensync/` is byte-for-byte as recorded, except `.kitchensync/runs.txt`, which has one more line, and no file was added there. Neither peer has a `.kitchensync` directory anywhere but its root.

## S-23: A Move Is Found Before The Walk Reaches The Old Path

Setup:

- `A/zz/movie.bin` exists with the 2 MiB content of S-19 and modification time `2024-01-01_10-00-00_000000Z`.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- On A, move `A/zz/movie.bin` to `A/aa/film.bin`. (`aa` sorts before `zz`, so the walk reaches the new path first.)

Action: run `released/kitchensync.exe --dry-run --verbosity info A B`, then `released/kitchensync.exe --verbosity info A B`.

Outcome: both exit 0 with empty stderr. The dry run's stdout is exactly `dry run`, `M2 aa/film.bin`, `sync complete`, and B is unchanged by it. The second run's stdout is exactly three lines: the rollback hint, `M2 aa/film.bin`, and `sync complete`. The user files under B are exactly `aa/film.bin` with the original content and modification time; `B/zz` is an empty directory, as `A/zz` is. B has no BAK.

## S-24: A File Displaced In An Earlier Run Is Moved Into Place

Setup:

- `A/movie.bin` exists with the 2 MiB content of S-19 and modification time `2024-01-01_10-00-00_000000Z`.
- First run `released/kitchensync.exe --verbosity error +A B` and require it to exit 0.
- On A, move `A/movie.bin` to `A/shows/film.bin`.
- Run `released/kitchensync.exe --verbosity error A B -x shows` and require it to exit 0. B's `movie.bin` is now in B's BAK, and B has no `shows`.

Action: run `released/kitchensync.exe --verbosity info A B`.

Outcome: the process exits 0 with empty stderr. stdout is exactly three lines: the rollback hint, `M2 shows/film.bin`, and `sync complete`. `B/shows/film.bin` holds the original content with modification time `2024-01-01_10-00-00_000000Z`. B's BAK holds no file.

## S-25: Per-Directory Manifests Are Read And Converted

Setup, writing manifests by hand in the per-directory layout (see state.md, "Per-directory manifests"), every user file with modification time `2024-01-01_10-00-00_000000Z`:

- `A/sub/keep.txt` and `B/sub/keep.txt` exist with bytes `keep\n`. `B/sub/gone.txt` exists with bytes `gone\n`; A has no `gone.txt`.
- On both peers, `.kitchensync/manifest.txt` holds the line `sub	d	2024-01-01_10-00-00_000000Z	-1	2024-02-01_10-00-00_000000Z	-	-`, and `sub/.kitchensync/manifest.txt` holds the lines `gone.txt	f	2024-01-01_10-00-00_000000Z	5	2024-02-01_10-00-00_000000Z	-	-` and `keep.txt	f	2024-01-01_10-00-00_000000Z	5	2024-02-01_10-00-00_000000Z	-	-` (fields separated by single tabs).
- `B/sub/.kitchensync/BAK/<T>/old.txt` exists with bytes `old\n`, where `<T>` is a timestamp one day before the run (recent enough not to expire), and beside it `._old.txt` with 4096 zero bytes (the litter macOS leaves on exFAT). `B/sub/.kitchensync/BAK/<T2>/manifest.txt`, with `<T2>` a microsecond after `<T>`, holds the line `keep.txt	f	2024-01-01_10-00-00_000000Z	5	2024-01-02_10-00-00_000000Z	-	-` (an archived manifest).

Action: run `released/kitchensync.exe --verbosity error A B`.

Outcome: the process exits 0. stdout is exactly `sync complete\n` (both peers have history, so there is no first-sync line). stderr is empty. The user files under both peers are exactly `sub/keep.txt`: B's `gone.txt` is displaced, because A's manifest shows A had it and lost it. Neither `A/sub/.kitchensync` nor `B/sub/.kitchensync` exists. Both peers have `.kitchensync/state.txt`. The files in B's BAK are exactly `sub/old.txt` with bytes `old\n`, in the directory `<T>`, and `sub/gone.txt` with bytes `gone\n`.

## S-26: A Newer State Format Is Left Alone

Setup:

- `A/one.txt` and `B/one.txt` exist with bytes `one\n` and modification time `2024-01-01_10-00-00_000000Z`.
- `B/.kitchensync/state.txt` holds the single line `#	99	2024-01-02_10-00-00_000000Z` (format 99, newer than any KitchenSync reads).
- `C/` exists and has no user files.

Action: run `released/kitchensync.exe --verbosity error A B C`.

Outcome: the process exits 0 with empty stderr. stdout is exactly three lines: `peer unreachable: <B>: io_error: state.txt is format 99, newer than this KitchenSync reads (format 1); use a newer KitchenSync`, where `<B>` is B as KitchenSync displays it (`file://` and its absolute path), then the first-sync line (A and C have no history), then `sync complete`. B's `.kitchensync/state.txt` is byte-for-byte unchanged, and B has no other file under `.kitchensync/`. `C/one.txt` holds `one\n`.

## S-27: A Deletion Wins Over The Same Version, Even Right After A Sync

Setup, every file written with the current time as its modification time (as a file just created or downloaded would have):

- `A/fresh.txt` exists with bytes `fresh\n`, and `A/edited.txt` with bytes `v1\n`. `B/` and `C/` exist and are empty.
- Run `released/kitchensync.exe --verbosity error +A B C` and require it to exit 0. All three peers now hold both files.
- Delete `A/fresh.txt`. Run `released/kitchensync.exe --verbosity error A B` (C is not part of this run) and require it to exit 0.
- Delete `A/edited.txt`. On C, rewrite `edited.txt` with bytes `v2 edited\n` and a modification time one minute in the future.

Action: run `released/kitchensync.exe --verbosity error A B C`.

Outcome: the process exits 0 with stdout exactly `sync complete\n` and empty stderr. No peer has `fresh.txt`: A deleted it in the earlier run, B lost it then, and C's copy is the version A deleted, so it goes too, although every time involved lies within a few seconds. All three peers hold `edited.txt` with bytes `v2 edited\n`: C edited it after A last saw it, so the edit survives A's deletion.

# Properties

## P-01: Output Channels

All KitchenSync output goes to stdout. stderr is empty for help, validation errors, successful syncs, and recoverable sync diagnostics.

## P-02: Copy Limit

At no point may more than `--parallel` file transfers hold active copy slots across the whole run, regardless of source scheme, destination scheme, peer, or host.

## P-03: Peer Metadata Is Never Synced

`.kitchensync/` and `.git/` entries, symbolic links, and special files are not part of the user file tree. They are omitted from listings, decisions, copies, and state entries unless a spec section explicitly describes direct metadata maintenance inside `.kitchensync/`.

## P-04: State Replacement Never Renames Over A Live File

`.kitchensync/state.txt` is replaced only by the sequence in state.md: write `state.txt.new`, move the live `state.txt` aside to `state.txt.old`, rename `state.txt.new` into place, then delete `state.txt.old`. No step ever renames onto an existing path, so the run works on SFTP servers that reject rename-over-existing, and the next normal run repairs any replacement that was interrupted at startup, before it reads the state.

## P-05: Dry Run Does Not Write Peer State

In `--dry-run`, KitchenSync connects, reads state, lists, decides, and prints the same progress lines, but it must not create, modify, rename, delete, or displace anything through a peer URL, and it writes no state, journal, or run log. File contents are not read and the copy queue is not exercised.
