# KitchenSync

Real-time directory synchronization across multiple filesystem targets.

## Why KitchenSync?

**Zero infrastructure on peers.** KitchenSync only needs SSH access to remote peers -- the same SSH you already use. No daemons to install, no ports to open, no services to manage. If you can `ssh user@host`, KitchenSync can sync with it.

**Run it however you want.** Keep it running all day with the filesystem watcher, or run it occasionally manually or from cron. Run it on one machine, or run it on several. KitchenSync adapts to your workflow, not the other way around.

**Occasionally-connected peers just work.** USB drives, laptops, NAS boxes that sleep, servers behind flaky connections -- plug them in or wake them up and KitchenSync brings them current. No manual intervention, no conflict dialogs, no "sync pending" limbo.

**Changes fan out in seconds.** The local database knows what changed since the last run. On startup, outgoing updates begin immediately -- no waiting to walk remote filesystems first.

**Highly parallel.** Up to 10 concurrent transfers per peer, with all peers syncing simultaneously. Utilizes fast links without manual tuning.

**Multiple paths to each peer.** Configure a peer with several connection methods (local file system, local IP, Tailscale, public DNS) and KitchenSync uses the first one that works. Automatically use home network when you're home, VPN when you're not.

**Never destructive.** KitchenSync keeps the old copy of every overwritten or deleted file. Displaced files go to `.kitchensync/BACK/` where you can recover them. Multiple changes are kept, but not forever. Deletions propagate with timestamp-based conflict resolution -- the most recent action wins, and ties go to keeping data.

**No central server.** Every peer is equal. Sync between any subset of your devices. Add or remove peers anytime. No account, no cloud, no subscription.

## How It Compares

|                          | KitchenSync             | rsync  | Syncthing           | Unison      |
| ------------------------ | ----------------------- | ------ | ------------------- | ----------- |
| Software needed on peers | SSH only                | rsync  | Syncthing daemon    | Unison      |
| Bidirectional            | Yes                     | No     | Yes                 | Yes         |
| Multi-peer mesh          | Yes                     | No     | Yes                 | Pairwise    |
| Deletion propagation     | Yes                     | Manual | Yes                 | Yes         |
| Watch mode               | Yes                     | No     | Always-on           | No          |
| Run occasionally         | Yes                     | Yes    | Not designed for it | Yes         |
| Conflict resolution      | Automatic (newest wins) | N/A    | Manual or LWW       | Interactive |

## Modes

**Watch mode** (default): Detects local changes immediately via filesystem watcher, syncs with all reachable peers continuously. Runs until stopped.

**Once mode** (`--once`): Syncs everything once and exits. Perfect for cron jobs, scripts, or "sync now" workflows.

## The `.kitchensync/` Directory

Each synced directory contains a `.kitchensync/` directory storing all metadata (excluded from sync):

| Path             | Purpose                                          |
| ---------------- | ------------------------------------------------ |
| `kitchensync.db` | SQLite database: file state, config, logs        |
| `peers.conf`     | Peer configuration                               |
| `PEER/`          | Peer databases with queues (persist across runs) |
| `BACK/`          | Displaced files, recoverable                     |

Transfer staging uses `.kitchensync/XFER/` directories throughout the tree, ensuring same-filesystem rename for instant swaps while keeping staging hidden from users.

## Cleanup

Old data is automatically cleaned up during walks. Retention periods are configurable in `peers.conf` (see `kitchensync --help`):

| What                      | Default Retention | Notes                                |
| ------------------------- | ----------------- | ------------------------------------ |
| Log entries               | 32 days           | Purged on every log insert           |
| `.kitchensync/XFER/` dirs | 2 days            | Incomplete transfers from crashes    |
| `BACK/` dirs              | 90 days           | Displaced files remain recoverable   |
| Tombstones                | 6 months          | Deletion records in the database     |
| `PEER/` databases         | 0 (startup)       | Databases for unlisted peers deleted |

## Peer Configuration

Peers are configured in `.kitchensync/peers.conf`. Each peer has a name followed by one or more URLs (tried in order):

```
nas
  sftp://bilbo@192.168.1.50/volume1/docs
  sftp://bilbo@nas.tail12345.ts.net/volume1/docs

laptop
  sftp://bilbo@laptop.local/home/bilbo/docs
  sftp://bilbo@laptop.tail12345.ts.net/home/bilbo/docs

usb-backup
  file:///media/bilbo/usb-backup/docs
```

## Timestamps

All timestamps use `YYYYMMDDTHHmmss.ffffffZ` format -- UTC with microsecond precision.

## Building

Written in Rust. Binaries go to `./released/`:

| Platform | Binary              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |
