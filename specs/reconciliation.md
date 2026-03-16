# Reconciliation

How KitchenSync resolves differences between two devices.

## Inputs

For each path, there are three possible states on each side:

- **Live file** — file exists on disk, with a `mod_time` and size (from the directory listing).
- **Deleted** — tombstone exists in SNAP, with a `del_time`.
- **Unknown** — no file on disk, no tombstone. This device has never seen this path.

## Decision Rules

Compare side A against side B for each path:

**Both sides live:**
- Same `mod_time` → no action
- Different `mod_time` → newer wins; loser displaced to `BACK/`, replaced via XFER

**One side live, other side deleted:**
- Compare `mod_time` against `del_time`; newer wins (see below)

**One side live, other side unknown:**
- Copy the file to the unknown side via XFER; create manifest entry

**Both sides deleted:** no action

**One side deleted, other side unknown:** no action

**Both sides unknown:** not possible (path wouldn't appear in either index)

## Live vs Deleted

When one side has a live file and the other has a deletion:

- **File is newer** (`mod_time` > `del_time`) → the file wins. It is copied to the side that deleted it (via XFER). The tombstone is removed.
- **Deletion is newer** (`del_time` > `mod_time`) → the deletion wins. The file is displaced to `BACK/`. A tombstone is created on that side.
- **Same timestamp** → the file wins. This biases toward preserving data.

## Transfer and Swap

When the decision rules determine that a file needs to be copied, the transfer is a four-phase process:

1. **Decide** — the decision rules identify that a file on side A should be written to side B. Record the current state.
2. **Transfer** — copy the file to side B's XFER staging area (`XFER/<uuid>/<timestamp>/<filename>`). This may be slow (large file over a network).
3. **Recheck** — stat the destination file on disk and check for a tombstone. If the destination changed since step 1 (someone else modified it, or another thread already placed a newer version), re-evaluate the decision rules. If the transfer is no longer warranted, abort — delete the XFER directory.
4. **Swap** — displace side B's existing file (if any) to `BACK/`, move the file from XFER to its final location, update side B's manifest, delete the XFER directory.

The recheck in step 3 is critical. A transfer over SFTP can take minutes. During that time, the destination may have changed. The recheck is cheap (one stat, one tombstone check) compared to the transfer.

## Displacing a File

Whenever a file loses, it is moved to:

```
.kitchensync/BACK/<timestamp>/<filename>
```

This works the same way on both sides. When displacing a file on a peer, the move happens on the peer's filesystem (over SFTP for remote peers).

## Symmetry

The decision rules are symmetric — swap A and B and the outcome is the same. There is no concept of "push" vs "pull." Reconciliation is bidirectional.
