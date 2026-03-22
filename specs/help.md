# Help Screen

`-h` or `--help` (or no arguments at all) prints the following text verbatim to stdout and exits 0. The text is embedded in the binary at build time.

```
Usage: kitchensync [--cfgdir <path>] [<url>...] [key=value...] [-h|--help]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See README.md for full docs.

Arguments:
  <url>         Peer URLs or local paths to sync. Trailing ! marks canon*.
  key=value     Settings (persisted to config file).
  --cfgdir path    Config directory (path required). Always ends with
                .kitchensync/ — appended if not already present.
                Example: --cfgdir ~ uses ~/.kitchensync/.
                Default: ~/.kitchensync/.

Quick start:
  kitchensync c:/photos! sftp://user@host/photos   First sync (local is canon)
  kitchensync c:/photos/ d:/backup/photos            Add another peer to group
  kitchensync c:/photos/                             Sync entire group

URL schemes:
  /path or c:\path                   Local (becomes file://)
  sftp://user@host/path              Remote over SSH
  sftp://user@host:port/path         Non-standard SSH port
  sftp://user:password@host/path     Inline password (prefer SSH keys)

Settings:
  max-connections=10               Max concurrent connections per URL
  connection-timeout=30            Seconds for SSH handshake timeout
  log-level=info                   Log level (error, info, debug, trace)
  xfer-cleanup-days=2              Delete stale staging dirs after N days
  back-retention-days=90           Delete displaced files after N days
  tombstone-retention-days=180     Forget deletion records after N days
  log-retention-days=32            Purge log entries after N days

Peer groups:
  Peers that appear together form a peer group. Specify any URL from
  a previous run to select the entire group. New URLs are added to
  the group. Groups and settings accumulate in the config file.

  Canon (!) is required on first sync (no snapshot history) but is not
  persisted — it applies to this run only. After the first sync,
  bidirectional sync works without canon. For permanent canon, edit
  the config file (set "canon": true on a peer entry).

Config directory (~/.kitchensync/ by default):
  kitchensync-conf.json    Accumulated config (peer groups, settings)
  kitchensync.db           Database (peer identity, snapshots)
  quartz.db                Database (instance state, logs)

No file is ever destroyed — displaced files go to .kitchensync/BACK/.

*"Canon" means source of truth; other peers will be made to look like the
 canon peer.
```
