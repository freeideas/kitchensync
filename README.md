# KitchenSync

Synchronize file trees across multiple peers.

## Why KitchenSync?

**Zero infrastructure on peers.** Only needs SSH access — no daemons, no ports, no services. If you can `ssh user@host`, KitchenSync can sync with it.

**Any peer, anywhere.** The local filesystem isn't special — it's just another peer. Sync two remote servers without touching the local machine, or include a local directory alongside remotes.

**Occasionally-connected peers just work.** USB drives, sleeping laptops, flaky connections — plug them in and KitchenSync brings them current. The snapshot knows what the world should look like; discrepancies are detected and resolved automatically.

**N-way sync in one pass.** All peers are listed in parallel at each directory level. Decisions are made once, copies fan out to all peers that need them.

**Multiple paths to every peer.** Each peer can have several URLs — local IP, VPN, Tailscale, public DNS. KitchenSync tries them in order and uses the first one that connects. At home you hit the NAS over your LAN; at the office it goes through the VPN. One config, no switching.

**Never destructive.** Old copies of overwritten or deleted files go to `.kitchensync/BACK/`. Multiple versions are kept for 90 days (configurable).

**No central server.** Every peer is equal (unless you use `--canon`). No account, no cloud, no subscription.

## How It Compares

|                          | KitchenSync             | rsync  | Syncthing           | Unison      |
| ------------------------ | ----------------------- | ------ | ------------------- | ----------- |
| Software needed on peers | No more than SSH        | rsync  | Syncthing daemon    | Unison      |
| Bidirectional            | Yes                     | No     | Yes                 | Yes         |
| Multi-peer mesh          | Yes                     | No     | Yes                 | Pairwise    |
| Deletion propagation     | Yes                     | Manual | Yes                 | Yes         |
| Conflict resolution      | Automatic (newest wins) | N/A    | Manual or LWW       | Interactive |

## Quick Start

1. Create a config directory and file:

   ```
   mkdir mydir/.kitchensync
   ```

2. Create `mydir/.kitchensync/kitchensync-conf.json`:

   ```json5
   {
     peers: {
       nas: {
         urls: [
           "sftp://bilbo@192.168.1.50/volume1/docs",   // home LAN
           "sftp://bilbo@nas.tail12345.ts.net/volume1/docs"  // VPN fallback
         ]
       },
       local: { urls: ["file://./"] }
     }
   }
   ```

3. Run:

   ```
   kitchensync mydir/
   ```

## Command Line

```
kitchensync <config> [--canon <peer-name>]
```

`<config>` can be:
- Path to a `.json` config file
- Path to a `.kitchensync/` directory
- Path to the parent of a `.kitchensync/` directory

`--canon <peer-name>` makes the named peer authoritative — every peer will be made the same as this named peer.

## The `.kitchensync/` Directory

Convention: place the config file and database in a `.kitchensync/` directory. Contents:

| Path                    | Purpose                                      |
| ----------------------- | -------------------------------------------- |
| `kitchensync-conf.json` | Peer configuration (JSON5)                   |
| `kitchensync.db`        | SQLite database: snapshot, config, logs      |
| `BACK/`                 | Displaced files, recoverable for 90 days     |

Transfer staging uses `.kitchensync/XFER/` directories near target files for atomic swaps.

## Cleanup

| What                      | Default Retention |
| ------------------------- | ----------------- |
| Log entries               | 32 days           |
| `.kitchensync/XFER/` dirs | 2 days            |
| `BACK/` dirs              | 90 days           |
| Tombstones                | 180 days          |

## Timestamps

All timestamps use `YYYYMMDDTHHmmss.ffffffZ` — UTC with microsecond precision.

## Building

Written in Rust. Binaries go to `./released/`:

| Platform | Binary              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |
