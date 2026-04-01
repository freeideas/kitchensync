package cli

// HelpText is the help screen, embedded at build time.
const HelpText = `Usage: kitchensync [options] <peer> [<peer>...]

Synchronize file trees across multiple peers.
One peer: snapshot only (record what's there, no sync).

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path, c:\path, or ./relative    Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon — this peer's state wins all conflicts
  -<peer>                          Subordinate — overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?mc=5"          Max connections for this URL
  "sftp://host/path?ct=60"         Connection timeout for this URL
  "sftp://host/path?mc=5&ct=60"    Both

Options:
  -h, --help, /?     Show this help
  -n, --dry-run      Show what would happen without making changes
  --mc N             Max concurrent connections per URL (default: 10)
  --ct N             SSH handshake timeout in seconds (default: 30)
  -vl LEVEL          Verbosity level: error, warn, info, debug, trace (default: info)
  --xd N             Delete stale TMP staging after N days; 0=never (default: 2)
  --bd N             Delete BAK/ files after N days; 0=never (default: 90)
  --td N             Forget deletion records after N days; 0=never (default: 180)

Quick start:
  kitchensync /mnt/usb/photos                         Snapshot only (no sync)
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://host/photos            Bidirectional
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from .kitchensync/BAK/ (kept for --bd days).`
