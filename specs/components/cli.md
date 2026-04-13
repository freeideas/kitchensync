# CLI

## Arguments

```
kitchensync [options] <peer> [<peer>...]
```

At least one peer is required. Peers are positional arguments. Options can appear anywhere.

### Peer Syntax

- `/path`, `c:\path`, `./relative` -- local path (treated as `file://`)
- `sftp://user@host/path` -- remote over SSH
- `sftp://user@host:port/path` -- non-standard SSH port
- `sftp://user:password@host/path` -- inline password (prefer SSH keys)

### Prefix Modifiers

- `+<peer>` -- canon: this peer's state wins all conflicts
- `-<peer>` -- subordinate: overwritten to match the group

At most one `+` peer is allowed. Any number of `-` peers.

### Fallback URLs

Multiple network paths to the same data, tried in order:

```
[url1,url2,...]
+[url1,url2,...]
-[url1,url2,...]
```

The prefix goes on the bracket, not on individual URLs.

### Per-URL Settings

Query-string parameters override global options for that URL:

```
"sftp://user@host/path?mc=5&ct=60"
```

Supported: `mc` (max connections), `ct` (connection timeout).

## Options

| Flag           | Description                                             | Default |
| -------------- | ------------------------------------------------------- | ------- |
| `-h, --help, /?` | Show help text and exit 0                            |         |
| `-n, --dry-run`  | Show what would happen without making changes         |         |
| `--watch`        | After initial sync, watch local peers for changes     |         |
| `--mc N`         | Max concurrent connections per URL                    | 10      |
| `--ct N`         | SSH handshake timeout in seconds                      | 30      |
| `-vl LEVEL`      | Verbosity: error, warn, info, debug, trace            | info    |
| `--xd N`         | Delete stale TMP staging after N days; 0=never        | 2       |
| `--bd N`         | Delete BAK/ files after N days; 0=never               | 90      |
| `--td N`         | Forget deletion records after N days; 0=never         | 180     |
| `--si N`         | Snapshot checkpoint interval in minutes               | 30      |

## Validation

- `--mc`, `--ct`, `--si` must be positive integers (>= 1)
- `--xd`, `--bd`, `--td` must be non-negative integers (>= 0); 0 means "never"
- `-vl` must be one of: error, warn, info, debug, trace
- At most one `+` (canon) peer
- At least one peer

On validation error: print a specific error message followed by the help text to stdout, exit 1.

## Help Text

`-h`, `--help`, `/?`, or no arguments at all prints the following text verbatim to stdout and exits 0:

```
Usage: kitchensync [options] <peer> [<peer>...]

Synchronize file trees across multiple peers.
One peer: snapshot only (record what's there, no sync).

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path, c:\path, or ./relative   Local path (same as file://)
  sftp://user@host/path           Remote over SSH
  sftp://user@host:port/path      Non-standard SSH port
  sftp://user:password@host/path  Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                         Canon -- this peer's state wins all conflicts
  -<peer>                         Subordinate -- overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                 Try in order, first that connects wins
  +[url1,url2,...]                Canon peer with fallbacks
  -[url1,url2,...]                Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://user@host/path?mc=5"         Max connections for this URL
  "sftp://user@host/path?ct=60"        Connection timeout for this URL
  "sftp://user@host/path?mc=5&ct=60"   Both

Options:
  -h, --help, /?     Show this help
  -n, --dry-run      Show what would happen without making changes
  --watch            After initial sync, watch local peers for changes
  --mc N             Max concurrent connections per URL (default: 10)
  --ct N             SSH handshake timeout in seconds (default: 30)
  -vl LEVEL          Verbosity: error, warn, info, debug, trace (def: info)
  --xd N             Delete stale TMP staging after N days; 0=never (def: 2)
  --bd N             Delete BAK/ files after N days; 0=never (default: 90)
  --td N             Forget deletion records after N days; 0=never (def: 180)
  --si N             Snapshot checkpoint interval in minutes (default: 30)

Quick start:
  kitchensync /mnt/usb/photos                         Snapshot only (no sync)
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://user@host/photos            Bidirectional
  kitchensync c:/photos sftp://user@host/photos -/mnt/usb  Add USB as subordinate
  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from .kitchensync/BAK/ (kept for --bd days).
```

The help text is embedded in the binary at build time.
