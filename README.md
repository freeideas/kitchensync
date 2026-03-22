# KitchenSync

Synchronize file trees across multiple peers.

## Why KitchenSync?

**Fast!** Changes propagate simultaneously across multiple concurrent connections. Peers are listed in parallel, decisions are made once, and copies fan out to all peers that need them. Written in Rust.

**Never lose files!** Old copies of overwritten or deleted files go to `.kitchensync/BAK/`. Multiple previous versions are kept for 90 days (configurable).

**Zero infrastructure on peers.** Only needs SSH access — no daemons, no ports, no services. If you can `ssh user@host`, KitchenSync can sync with it.

**Fully command-line driven.** No config files, no setup wizards, no JSON. Just URLs on a command line. Want a fancy config file? Write a shell script. Or don't — it's up to you.

**Occasionally-connected peers just work.** USB drives, sleeping laptops, flaky connections — plug them in and KitchenSync brings them current. Each peer carries its own snapshot history, so it always knows what changed.

**Multiple paths to every peer.** Each peer can have several fallback URLs — local IP, VPN, Tailscale, public DNS. KitchenSync tries them in order and uses the first one that connects. At home you hit the cloud drive over your LAN; at the office it goes through the VPN. One command, no switching.

**Native on Windows, Linux, and macOS.** A single binary, no dependencies. No Cygwin, no WSL, no MinGW — just download and run. Paths like `c:\photos` work exactly as you'd expect.

**No central server.** Every peer is equal (unless you say otherwise). No account, no cloud, no subscription.

## The `+` and `-` URL Prefixes

- **`+`** — this peer wins every disagreement (canon)
- **`-`** — this peer loses every disagreement (subordinate)
- **(no prefix)** — bidirectional; newest wins

## Quick Start

Any peer without a snapshot is automatically treated as `-` — it receives the group's state without influencing decisions.

Sync your photos to a cloud drive. First time, use `+` so your local copy wins every disagreement:

```
kitchensync +c:/photos sftp://bilbo@cloud/volume1/photos
```

That's it. Both directories are now in sync. No config file was created. No database was installed anywhere central. Each peer just got a tiny `.kitchensync/` directory with its snapshot.

## Next Time

Run the same thing without `+`. KitchenSync uses the snapshots from last time to sync bidirectionally — changes on either side propagate. You don't have to remember which files you changed on which device; KitchenSync knows what to update, copy, and/or archive.

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos
```

## Add More Peers

Just add them to the command:

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos d:/backup/photos
```

The new peer has no snapshot yet, so it's automatically subordinate — it receives the group's state without influencing decisions.

## Add a USB Drive

Use `-` to explicitly mark a peer as subordinate, even if it already has a snapshot:

```
kitchensync c:/photos sftp://bilbo@cloud/volume1/photos -/mnt/usb/photos
```

Next time the USB is plugged in, drop the `-` and it participates as a full bidirectional peer.

## Fallback URLs

Your cloud drive has a local IP and a VPN address? Group them with brackets — KitchenSync tries each in order:

```
kitchensync c:/photos [h:/office-share/photos,sftp://192.168.1.50:2222/photos,sftp://cloud.vpn/photos]
```

At home it connects over LAN. At the office it falls back to VPN. One command.

## Per-URL Tuning

Slow VPN link? Limit its connections. Fast LAN? Crank them up. Use query-string parameters:

```
kitchensync c:/photos "[sftp://192.168.1.50/photos?mc=20,sftp://cloud.vpn/photos?mc=3&ct=60]"
```

(Quotes needed because of the `?` — your shell would glob it otherwise.)

## Global Options

Set defaults for the whole run:

```
kitchensync --mc 5 --ct 60 c:/photos sftp://host/photos
```

| Flag   | Default | Meaning                                     |
| ------ | ------- | ------------------------------------------- |
| `--mc` | 10      | Max concurrent connections per URL          |
| `--ct` | 30      | SSH handshake timeout (seconds)             |
| `-vl`  | `info`  | Verbosity level (error, info, debug, trace) |
| `--xd` | 2       | Delete stale staging after N days           |
| `--bd` | 90      | Delete displaced files after N days         |
| `--td` | 180     | Forget deletion records after N days        |

## How It Compares

|                           | KitchenSync             | rsync        | Syncthing        | Unison       |
| ------------------------- | ----------------------- | ------------ | ---------------- | ------------ |
| Deleted/Overwritten files | Recoverable for a while | LOST FOREVER | LOST FOREVER     | LOST FOREVER |
| Needed on peers           | SSH or nothing          | SSH + rsync  | Syncthing daemon | SSH + Unison |
| Bidirectional             | Yes                     | No           | Yes              | Yes          |
| Multi-peer mesh           | Yes                     | No           | Yes              | Tricky       |
| Delete propagation        | Yes                     | Opt-in       | Yes              | Yes          |
| Conflict resolution       | Newest Wins             | Overwrite    | Configurable     | Interactive  |
| Config files              | No                      | No           | OMG Yes          | Yes          |
| Windows support           | Excellent               | Tricky       | Excellent        | OK           |

## URL Schemes

| Form                                 | Meaning                           |
| ------------------------------------ | --------------------------------- |
| `/path` or `c:\path` or `./relative` | Local path (same as `file://`)    |
| `sftp://user@host/path`              | Remote over SSH (port 22)         |
| `sftp://user@host:port/path`         | Non-standard SSH port             |
| `sftp://user:password@host/path`     | Inline password (prefer SSH keys) |

## Authentication

For remote peers, just make sure you can reach the directory via SSH. If `ssh user@host` `cd /path` works, KitchenSync can sync it.

KitchenSync tries these in order:

1. Inline password from URL
2. SSH agent (`SSH_AUTH_SOCK`)
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_ecdsa`
5. `~/.ssh/id_rsa`

Host keys verified via `~/.ssh/known_hosts`. Unknown hosts rejected.

## The `.kitchensync/` Directory

The snapshot database lives at the peer root. BAK/ and TMP/ directories are created alongside affected files at any directory level.

| Path                                                        | Purpose                          |
| ----------------------------------------------------------- | -------------------------------- |
| `.kitchensync/snapshot.db`                                  | Peer's snapshot history (SQLite) |
| `<parent>/.kitchensync/BAK/<timestamp>/<basename>`          | Displaced files (recoverable)    |
| `<parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>`   | Transfer staging (atomic swap)   |

These are never synced between peers.

## How Sync Works

1. Connect to all peers in parallel (fallback URLs tried in order)
2. Download each peer's snapshot to a local temp directory
3. Walk the combined directory tree, listing all peers in parallel at each level
4. Union the entries across peers
5. For each entry, decide the authoritative state (canon wins, or newest mod_time wins)
6. Enqueue file copies, create/remove directories as needed
7. Execute copies concurrently (subject to connection limits)
8. Upload updated snapshots back to each peer (atomic rename)

Decisions are made once per entry, not per peer pair. Snapshots track what each peer had last time, enabling deletion detection and conflict resolution.

## Building

Written in Rust. Binaries go to `./released/`:

| Platform | Binary              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |
