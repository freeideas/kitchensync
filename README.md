# KitchenSync

Synchronize file trees across multiple peers.

## Why KitchenSync?

**Zero infrastructure on peers.** Only needs SSH access — no daemons, no ports, no services. If you can `ssh user@host`, KitchenSync can sync with it.

**Any peer, anywhere.** The local filesystem isn't special — it's just another peer. Sync two remote servers without touching the local machine, or include a local directory alongside remotes.

**Occasionally-connected peers just work.** USB drives, sleeping laptops, flaky connections — plug them in and KitchenSync brings them current. The snapshot knows what the world should look like; discrepancies are detected and resolved automatically.

**N-way sync in one pass.** All peers are listed in parallel at each directory level. Decisions are made once, copies fan out to all peers that need them.

**Multiple paths to every peer.** Each peer can have several fallback URLs — local IP, VPN, Tailscale, public DNS. KitchenSync tries them in order and uses the first one that connects. At home you hit the NAS over your LAN; at the office it goes through the VPN. One config, no switching.

**Never destructive.** Old copies of overwritten or deleted files go to `.kitchensync/BACK/`. Multiple versions are kept for 90 days (configurable).

**No central server.** Every peer is equal (unless you mark one as canon). No account, no cloud, no subscription.

**Quick one-off syncs from the command line.** Define peers inline — no config file needed. `kitchensync c:/photos! sftp://host/photos` syncs right now, remembers the group for next time.

## How It Compares

|                          | KitchenSync             | rsync  | Syncthing        | Unison      |
| ------------------------ | ----------------------- | ------ | ---------------- | ----------- |
| Software needed on peers | No more than SSH        | rsync  | Syncthing daemon | Unison      |
| Bidirectional            | Yes                     | No     | Yes              | Yes         |
| Multi-peer mesh          | Yes                     | No     | Yes              | Pairwise    |
| Deletion propagation     | Yes                     | Manual | Yes              | Yes         |
| Conflict resolution      | Automatic (newest wins) | N/A    | Manual or LWW    | Interactive |

## Quick Start

**First sync — two peers, local is canon:**

```
kitchensync c:/photos! sftp://bilbo@nas/volume1/photos
```

This syncs `c:/photos/` to the NAS. The `!` marks the local directory as canon (its state wins conflicts). A peer group is created and remembered.

**Add another peer to the same group:**

```
kitchensync c:/photos/ d:/backup/photos
```

KitchenSync recognizes `c:/photos/` from the previous run, finds its group, and adds `d:/backup/photos` as a new peer. All three peers are now synced.

**Run the group again (just name any URL in it):**

```
kitchensync c:/photos/
```

KitchenSync looks up the group containing `c:/photos/`, finds all its peers (`c:/photos/`, `sftp://bilbo@nas/volume1/photos`, `d:/backup/photos`), and syncs them all.

## Command Line

```
kitchensync [--cfg [<path>]] <url>... [key=value...] [-h|--help]
```

No arguments prints help.

- **`<url>`** — peer URLs or local paths. Each is a peer to sync. Bare paths (no `file://` prefix) are treated as local `file://` URLs. A trailing `!` marks the URL as canon.
- **`key=value`** — settings persisted to the config file (see Settings below).
- **`--cfg [<path>]`** — config directory. If `<path>` ends with `.kitchensync/` or `.kitchensync`, it is used as-is (with a trailing `/` added if absent). Otherwise, `.kitchensync/` is appended. Default (or `--cfg` alone): `~/.kitchensync/`.

### How arguments are parsed

Arguments without `=` are peer URLs. Arguments with `=` are settings. This is unambiguous because URLs never contain `=`.

### URL schemes

| Form                                 | Meaning                           |
| ------------------------------------ | --------------------------------- |
| `/path` or `c:\path` or `./relative` | Local path (becomes `file://`)    |
| `sftp://user@host/path`              | Remote over SSH (port 22)         |
| `sftp://user@host:port/path`         | Non-standard SSH port             |
| `sftp://user:password@host/path`     | Inline password (prefer SSH keys) |

Percent-encode special characters in SFTP passwords (`@` → `%40`, `:` → `%3A`). SFTP paths are absolute from filesystem root.

### Authentication (fallback chain)

1. Inline password from URL
2. SSH agent (`SSH_AUTH_SOCK`)
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_ecdsa`
5. `~/.ssh/id_rsa`

Host keys verified via `~/.ssh/known_hosts`. Unknown hosts rejected.

## Peer Groups

A peer group is a set of peers that synchronize with each other. Groups are the core organizing concept in KitchenSync.

### How groups form

When you run `kitchensync url1 url2`, both URLs are placed in the same peer group. If either URL already belongs to an existing group, the other is added to it.

### How groups are recognized

Every URL is normalized and stored in the database. On the next run, specifying any single URL from a group selects the entire group. You don't need to list all peers every time.

### URL normalization

URLs are normalized before storage and lookup: scheme and hostname are lowercased, default ports are removed, consecutive slashes are collapsed, trailing slashes are removed, and bare paths are resolved to absolute `file://` URLs.

### Group conflicts

If you specify URLs that belong to two different existing groups, that's an error. To merge groups, edit the config file (`~/.kitchensync/kitchensync-conf.json`).

## Canon Peer

A canon peer is authoritative — its state wins all conflicts unconditionally.

The `!` suffix on the command line marks a peer as canon **for this run only** — it is not persisted to the config file:

```
kitchensync c:/photos! sftp://host/photos
```

**Canon is required on the first sync** of a new group (when no snapshot history exists). Without snapshot history, KitchenSync can't tell which files are new vs deleted, so it needs one peer to be the source of truth.

**After the first sync**, snapshot history exists and bidirectional sync works without canon. You can drop the `!`:

```
kitchensync c:/photos/
```

For **permanent canon** (every run treats a peer as authoritative), edit the config file and set `"canon": true` on the peer entry. At most one peer per group may be canon.

## Config Directory

Default: `~/.kitchensync/`. Override with `--cfg <path>`.

Contains two fixed-name files:

| File                    | Purpose                                  |
| ----------------------- | ---------------------------------------- |
| `kitchensync-conf.json` | Accumulated config (peer groups, settings) |
| `kitchensync.db`        | SQLite database (snapshots, logs, state) |

### Config file accumulation

The config file accumulates state across runs. Every CLI setting and URL is merged into the file and persisted. If you run:

```
kitchensync c:/photos/ sftp://host/photos max-connections=5
```

The next run inherits `max-connections=5` and knows about both peers, even if you only specify one URL.

### Config file format

The config file is JSON with `//` and `/* */` comments allowed. Comments are stripped before parsing.

```json5
{
  // Global settings
  "max-connections": 10,
  "connection-timeout": 30,
  "xfer-cleanup-days": 2,
  "back-retention-days": 90,
  "tombstone-retention-days": 180,
  "log-retention-days": 32,

  // Peer groups
  "peer_groups": [
    {
      "name": "photos",
      "peers": [
        { "name": "local", "urls": ["file:///c:/photos"], "canon": true },
        { "name": "nas", "urls": ["sftp://bilbo@nas/volume1/photos"] },
        { "name": "backup", "urls": ["file:///d:/backup/photos"] }
      ]
    },
    {
      "name": "docs",
      "peers": [
        { "name": "laptop", "urls": ["file:///home/bilbo/docs"] },
        { "name": "nas", "urls": [
            "sftp://bilbo@192.168.1.50/docs",
            { "url": "sftp://bilbo@nas.vpn/docs", "max-connections": 3, "connection-timeout": 60 }
          ]
        }
      ]
    }
  ]
}
```

### Fallback URLs

A peer can have multiple URLs in its `urls` list — these are different network paths to the same data (e.g., LAN IP vs VPN hostname). They share one peer identity and snapshot history. URLs are tried in order; the first that connects wins. On the CLI, each URL argument is a separate peer. Multiple fallback URLs per peer are a config-file feature.

## Settings

| Setting                    | Default | Meaning                                       |
| -------------------------- | ------- | --------------------------------------------- |
| `max-connections`          | 10      | Max concurrent connections per URL            |
| `connection-timeout`       | 30      | Seconds for SSH handshake timeout             |
| `log-level`                | `info`  | Log level (`error`, `info`, `debug`, `trace`) |
| `xfer-cleanup-days`        | 2       | Delete stale staging dirs after N days        |
| `back-retention-days`      | 90      | Delete displaced files after N days           |
| `tombstone-retention-days` | 180     | Forget deletion records after N days          |
| `log-retention-days`       | 32      | Purge log entries after N days                |

Settings can be specified on the CLI as `key=value` or in the config file. CLI values are merged into the config file and persisted.

## The `.kitchensync/` Directory (in peer trees)

Each peer's file tree may contain `.kitchensync/` directories for operational data:

| Path                                              | Purpose                        |
| ------------------------------------------------- | ------------------------------ |
| `.kitchensync/BACK/<timestamp>/<basename>`        | Displaced files (recoverable)  |
| `.kitchensync/XFER/<timestamp>/<uuid>/<basename>` | Transfer staging (atomic swap) |

These are created near the affected files throughout the tree. `.kitchensync/` directories are never synced between peers.

## How Sync Works

1. List all peers' directories in parallel at each level
2. Union the entries across peers
3. For each entry, decide the authoritative state (canon wins, or newest mod_time wins)
4. Enqueue file copies, create/remove directories as needed
5. Execute copies concurrently (subject to connection limits)
6. Update snapshot in the database

Decisions are made once per entry, not per peer pair. The snapshot tracks what each peer had last time, enabling deletion detection and conflict resolution.

## Peer Identity in the Database

Each peer is assigned a stable integer ID, and its URLs are stored in a lookup table. Snapshot rows (file state) are keyed by this peer ID. On every startup, the database's peer/URL tables are reconciled against the config file in two passes: first recognize known URLs (read-only), then rewrite the URL mappings to match the config.

This means:
- Specifying any URL from a group reconstitutes the entire group
- Adding a new URL alongside a known one adds it to the existing group
- Renaming a URL in the config preserves all snapshot history (same peer ID)
- Reorganizing groups in the config is a free operation — snapshot data is per-peer, not per-group
- Fallback URLs (multiple paths to the same data) share one peer ID

## Building

Written in Rust. Binaries go to `./released/`:

| Platform | Binary              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |
