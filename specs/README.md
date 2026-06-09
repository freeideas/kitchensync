# KitchenSync

Synchronize file trees across multiple peers.

KitchenSync is for people who keep the same files in more than one place:
photos on a laptop and a NAS, project archives on a workstation and a USB
drive, or shared folders reachable through different network paths. It is a
plain command-line tool, not a service. If a peer is reachable by local
filesystem access or SSH/SFTP, KitchenSync can bring it into the group.

## How to run

Run the released CLI executable as `kitchensync` with options followed by two or
more peer paths or URLs:

```
kitchensync [options] <peer> <peer> [<peer>...]
```

Tests observe the process exit code, stdout, stderr, and filesystem changes
under the peer directories they create. All diagnostics and progress output go
to stdout; stderr remains empty.

### Released artifacts

The build writes its shipped artifact under `./released/`. `./released/`
contains exactly one file:

- `released/kitchensync.exe` - the CLI executable described above. The `.exe`
  suffix is used on every platform, including Linux and macOS where it is not
  conventional.

The build produces this file and tests invoke it directly from `./released/`.

## Why KitchenSync?

**Concurrent by design.** Peers are listed in parallel, decisions are made once,
and file copies fan out under one global copy limit.

**Recover overwritten and deleted files.** Replaced or removed content is moved
near the affected path so a bad sync is recoverable without a central server.

**No KitchenSync infrastructure on peers.** There are no daemons, open ports,
accounts, subscriptions, or cloud control planes.

**Command-line driven.** Peer lists and run options come from the command. Saved
workflows can be ordinary shell scripts.

**Occasionally connected peers work naturally.** A sleeping laptop or unplugged
USB drive can miss a run and catch up later from its own snapshot history.

**Multiple paths can identify one peer.** A peer can be tried through a local
path, LAN address, VPN name, or public DNS name, in the order supplied by the
command.

## First Sync

The first run needs one authoritative peer:

```
kitchensync +c:/photos sftp://bilbo@cloud/volume1/photos
```

The `+` marks the copy that should win disagreements. After the first run, the
same peers can sync bidirectionally:

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos
```

## Add A Peer

Add another peer to the command. New peers receive the group's current state
before they influence future decisions:

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos d:/backup/photos
```

A peer can also be marked subordinate for a run:

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos -/mnt/usb/photos
```

## Fallback Paths

Put multiple URLs in brackets when they are different ways to reach the same
peer:

```
kitchensync c:/photos [h:/office-share/photos,sftp://192.168.1.50:2222/photos,sftp://cloud.vpn/photos]
```

Connection tuning can be attached to individual URLs:

```
kitchensync c:/photos "[sftp://192.168.1.50/photos?timeout-conn=20,sftp://cloud.vpn/photos?timeout-conn=60&timeout-idle=10]"
```

## Exclude A Path

Use `-x` for paths that should be ignored during a run:

```
kitchensync c:/appz d:/appz -x "PortablePlatform/PortableApps" -x brave
```

## Specification

The detailed behavior lives in the focused specs:

- `sync.md` defines the command line, startup, run phases, transports, logging,
  dry-run behavior, and error handling.
- `multi-tree-sync.md` defines traversal, decisions, excludes, subordinate
  peers, BAK/TMP cleanup, and snapshot updates during sync.
- `database.md` defines peer snapshot storage, schema, URL normalization, path
  hashing, tombstones, and timestamps.
- `concurrency.md` defines copy concurrency, fallback connection behavior,
  listing concurrency, progress output, retries, and trace logging.
- `help.md` defines the exact help screen.
- `TESTING-GUIDELINES.md` defines constraints for SFTP tests.
