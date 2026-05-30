# Help Screen

No arguments prints the following text verbatim to stdout and exits 0. Output goes to stdout only; stderr is empty. Argument validation errors on non-help invocations (too few peers, multiple `+` peers, unrecognized flags, invalid values) print a specific error message followed by the help text, and exit 1.

```
Usage: kitchensync [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See the specs for full behavior.

Peers:
  /path or c:\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://host/path                 Remote over SSH, current OS user
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon - this peer's state wins all conflicts
  -<peer>                          Subordinate - overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?timeout-conn=60"     Connection timeout for this URL
  "sftp://host/path?timeout-idle=10"     SFTP idle keep-alive TTL for this URL
  "sftp://host/path?timeout-conn=60&timeout-idle=10"  Combine multiple

Options:
  --dry-run          Read-only and plan, but make no peer changes
  --max-copies N     Max active file copies across the whole run (default: 10)
  --retries-copy N   Give up copying after this many tries (default: 3)
  --retries-list N   Give up listing after this many tries (default: 3)
  --timeout-conn N   SSH handshake timeout in seconds (default: 30)
  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)
  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)
  -x RELPATH         Exclude relative slash path from sync; repeatable
  --keep-tmp-days N  Delete stale TMP staging after N days (default: 2)
  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)
  --keep-del-days N  Forget deletion records after N days (default: 180)

Quick start:
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://host/photos            Bidirectional
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from nearby:
  .kitchensync/BAK/ directories (kept for --keep-bak-days days).
```
