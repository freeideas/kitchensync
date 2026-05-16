# KitchenSync

Synchronize file trees across multiple peers.

## Why KitchenSync?

**Concurrent by design.** Changes propagate simultaneously across multiple concurrent connections. Peers are listed in parallel, decisions are made once, and copies fan out to all peers that need them.

**Recover overwritten and deleted files.** Old copies of overwritten or deleted files go to a `.kitchensync/BAK/` directory beside the affected path. Multiple previous versions are kept for 90 days (configurable).

**Zero KitchenSync infrastructure on peers.** Only needs SSH access — no KitchenSync daemons, ports, or services. If you can `ssh user@host`, KitchenSync can sync with it.

**Fully command-line driven.** Peer lists and run options come from the command line. No setup wizards, no JSON. Want a fancy saved command? Write a shell script. Or don't — it's up to you.

**Occasionally-connected peers just work.** USB drives, sleeping laptops, flaky connections — plug them in and KitchenSync brings them current. Each peer carries its own snapshot history, so it always knows what changed.

**Multiple paths to every peer.** Each peer can have several fallback URLs — local IP, VPN, Tailscale, public DNS. KitchenSync tries them in order and uses the first one that connects. At home you hit the cloud drive over your LAN; at the office it goes through the VPN. One command, no switching.

**Runs anywhere Java does.** A single JAR plus a Java 21 runtime — Windows, Linux, macOS. No Cygwin, no WSL, no MinGW. Paths like `c:\photos` work exactly as you'd expect.

**No central server.** Every peer is equal (unless you say otherwise). No account, no cloud, no subscription.

## The `+` and `-` URL Prefixes

- **`+`** — this peer wins every disagreement (canon)
- **`-`** — this peer loses every disagreement (subordinate)
- **(no prefix)** — bidirectional; newest wins

## Quick Start

Any non-canon peer without a snapshot is automatically treated as `-` — it receives the group's state without influencing decisions.

Sync your photos to a cloud drive. First time, use `+` so your local copy wins every disagreement:

```
java -jar kitchensync.jar +c:/photos sftp://bilbo@cloud/volume1/photos
```

That's it. Both directories are now in sync. No config file was created. No database was installed anywhere central. Each peer just got a tiny `.kitchensync/` directory with its snapshot.

## Next Time

Run the same thing without `+`. KitchenSync uses the snapshots from last time to sync bidirectionally — changes on either side propagate. You don't have to remember which files you changed on which device; KitchenSync knows what to update, copy, and/or archive.

```
java -jar kitchensync.jar c:/photos sftp://bilbo@cloud/volume1/photos
```

## Add More Peers

Just add them to the command:

```
java -jar kitchensync.jar c:/photos sftp://bilbo@cloud/volume1/photos d:/backup/photos
```

The new peer has no snapshot yet, so it's automatically subordinate — it receives the group's state without influencing decisions.

## Add a USB Drive

Use `-` to explicitly mark a peer as subordinate, even if it already has a snapshot:

```
java -jar kitchensync.jar c:/photos sftp://bilbo@cloud/volume1/photos -/mnt/usb/photos
```

Next time the USB is plugged in, drop the `-` and it participates as a full bidirectional peer.

## Fallback URLs

Your cloud drive has a local IP and a VPN address? Group them with brackets — KitchenSync tries each in order:

```
java -jar kitchensync.jar c:/photos [h:/office-share/photos,sftp://192.168.1.50:2222/photos,sftp://cloud.vpn/photos]
```

At home it connects over LAN. At the office it falls back to VPN. One command.

## Per-URL Tuning

Slow VPN link? Limit its connections. Fast LAN? Crank them up. Use query-string parameters:

```
java -jar kitchensync.jar c:/photos "[sftp://192.168.1.50/photos?mc=20,sftp://cloud.vpn/photos?mc=3&ct=60]"
```

(Quotes needed because of the `?` — your shell would glob it otherwise.)

## Global Options

Set defaults for the whole run:

```
java -jar kitchensync.jar --mc 5 --ct 60 c:/photos sftp://host/photos
```

| Flag   | Default | Meaning                                     |
| ------ | ------- | ------------------------------------------- |
| `--mc` | 10      | Max SFTP connections per user+host+port     |
| `--ct` | 30      | SSH handshake timeout (seconds)             |
| `--ka` | 30      | SFTP idle keep-alive TTL (seconds)          |
| `-vl`  | `info`  | Verbosity level (error, info, debug, trace) |
| `--xd` | 2       | Delete stale staging after N days           |
| `--bd` | 90      | Delete displaced files after N days         |
| `--td` | 180     | Forget deletion records after N days        |

## URL Schemes

| Form                                 | Meaning                           |
| ------------------------------------ | --------------------------------- |
| `/path` or `c:\path` or `./relative` | Local path (same as `file://`)    |
| `sftp://user@host/path`              | Remote over SSH (port 22)         |
| `sftp://user@host:port/path`         | Non-standard SSH port             |
| `sftp://host/path`                   | Remote over SSH, current OS user  |
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

| Path                                                      | Purpose                          |
| --------------------------------------------------------- | -------------------------------- |
| `.kitchensync/snapshot.db`                                | Peer's snapshot history (SQLite) |
| `<parent>/.kitchensync/BAK/<timestamp>/<basename>`        | Displaced files (recoverable)    |
| `<parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>` | Transfer staging (atomic swap)   |

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
