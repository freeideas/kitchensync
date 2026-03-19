# Help Screen

`-h` or `--help` prints the following text verbatim to stdout and exits 0. The text is embedded in the binary at build time.

```
Usage: kitchensync <config> [OPTIONS]

Synchronize file trees across multiple peers.

Arguments:
  <config>  Path to config file, .kitchensync/ directory, or parent directory.

Options:
      --canon <peer>  Named peer is authoritative (its state always wins)
  -h, --help          Print this help

Config file resolution:
  1. Path to a .json file         -> use directly
  2. Path to a .kitchensync/ dir  -> append kitchensync-conf.json
  3. Path to any other dir        -> append .kitchensync/kitchensync-conf.json

Path resolution:
  All relative paths in the config resolve from the config file's directory.
  Peer URLs have one extra rule: .kitchensync/ can never be a sync target.
  If the config is inside a .kitchensync/ directory, peer URL paths back up
  to the parent of .kitchensync/ (so "." becomes ".."). Config at
  mydir/.kitchensync/kitchensync-conf.json with peer URL file://./
  refers to mydir/. This adjustment applies only to peer URLs, not to
  other settings like "database".

Setup:
  1. mkdir mydir/.kitchensync
  2. Create mydir/.kitchensync/kitchensync-conf.json:

     {
       peers: {
         nas:   { urls: ["sftp://user@host/path"] },
         local: { urls: ["file://./"] }
       }
     }

  3. Run: kitchensync mydir/

Full example config with all settings at their defaults (JSON5):

  {
    database: "kitchensync.db",       // SQLite database path, relative to config dir
    connection-timeout: 30,           // seconds for SSH connect to be aborted
    max-reads: 10,                    // max concurrent reads per peer
    max-writes: 10,                   // max concurrent writes per peer
    xfer-cleanup-days: 2,             // delete stale staging dirs after N days
    back-retention-days: 90,          // delete displaced files after N days
    tombstone-retention-days: 180,    // forget deletion records after N days
    log-retention-days: 32,           // purge log entries after N days

    // Peers: at least two required. URLs tried top-to-bottom; first success wins.
    peers: {
      nas: {
        urls: [
          "sftp://bilbo@192.168.1.50/volume1/docs",
          "sftp://bilbo@nas.tail12345.ts.net/volume1/docs"
        ]
      },
      laptop: {
        urls: [
          "sftp://bilbo@laptop.local/home/bilbo/docs",
          "sftp://bilbo@laptop.tail12345.ts.net/home/bilbo/docs"
        ]
      },
      usb: {
        urls: ["file:///media/bilbo/usb-backup/docs"]
      }
    }
  }

URL schemes:
  sftp://user@host/path              Remote over SSH (port 22)
  sftp://user@host:port/path         Non-standard SSH port
  sftp://user:password@host/path     Inline password (prefer SSH keys)
  file:///absolute/path              Local, absolute
  file://./relative/path             Local, relative to config dir

  Percent-encode special characters in passwords (@ -> %40, : -> %3A).
  SFTP paths are absolute from filesystem root.

Authentication (fallback chain, stops at first success):
  1. Inline password from URL
  2. SSH agent (SSH_AUTH_SOCK)
  3. ~/.ssh/id_ed25519
  4. ~/.ssh/id_ecdsa
  5. ~/.ssh/id_rsa

  Host keys verified via ~/.ssh/known_hosts. Unknown hosts rejected.

Peer names:
  Must match [a-zA-Z0-9][a-zA-Z0-9_-]*, max 64 characters.
```
