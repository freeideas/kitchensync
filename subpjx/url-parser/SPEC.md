# URL Parser

A Java 21 library for parsing peer URL operands into structured peer roles,
ordered fallback URL candidates, per-URL settings, authentication fields, and
canonical URL identities.

The library is for URL and operand parsing only. It does not parse global
command-line flags, print help, open files, create directories, connect to
servers, authenticate, verify host keys, choose a reachable fallback URL,
perform synchronization decisions, copy data, manage snapshots, apply ignore
rules, schedule work, or log diagnostics. Callers provide the raw peer operand
text plus process context such as the current working directory and current OS
user, then use the parsed result outside this library.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`ParseContext`

| Field | Meaning |
| --- | --- |
| `current_working_directory` | Absolute local directory used to resolve relative bare paths and relative `file://` paths. |
| `current_os_user` | Username inserted into `sftp://host/path` URLs that omit a username. |

`PeerRole`

| Value | Meaning |
| --- | --- |
| `canon` | The operand had a leading `+`. |
| `subordinate` | The operand had a leading `-`. |
| `normal` | The operand had no role prefix. |

`UrlScheme`

| Value | Meaning |
| --- | --- |
| `file` | Local filesystem URL or bare path. |
| `sftp` | Remote filesystem over SFTP. |

`UrlSettings`

All fields are optional. Missing fields mean the caller should use its own
defaults.

| Field | Query parameter | Meaning |
| --- | --- | --- |
| `max_connections` | `mc` | Maximum SFTP transfer connections for the URL's user, host, and port. |
| `connect_timeout_seconds` | `ct` | SSH handshake timeout in seconds. |
| `idle_keep_alive_seconds` | `ka` | SFTP idle keep-alive TTL in seconds. |

`ParsedUrl`

| Field | Meaning |
| --- | --- |
| `scheme` | `file` or `sftp`. |
| `canonical_identity` | Normalized URL string used for equality, lookup, and diagnostics. Query settings and passwords are excluded. |
| `settings` | Parsed query-string settings. |
| `path` | Absolute path for `file`; absolute remote path for `sftp`. Uses `/` separators and has no trailing slash unless it is the root path. |
| `user` | Present only for `sftp`, with omitted usernames filled from `current_os_user`. |
| `password` | Optional decoded inline password for `sftp`. |
| `host` | Present only for `sftp`, lowercased. |
| `port` | Present only for `sftp`; omitted/default ports are normalized to `22`. |
| `endpoint_key` | Present only for `sftp`, formatted as `user@host:port`. The path and password are not part of the endpoint key. |

`ParsedPeer`

| Field | Meaning |
| --- | --- |
| `role` | Parsed peer role. |
| `candidates` | One or more `ParsedUrl` values in fallback order. A non-bracket operand has exactly one candidate. |

### Operations

`PeerUrlParser.parse_peer_operand(text, context) -> ParsedPeer`

Parses one peer operand. A leading `+` or `-` before the operand sets the peer
role. Bracket syntax groups fallback URLs:

```text
[sftp://192.0.2.10/photos,sftp://backup.example/photos]
```

The role prefix applies to the whole bracket group. Role prefixes inside a
bracket group are invalid.

`PeerUrlParser.parse_url(text, context) -> ParsedUrl`

Parses one URL candidate with no peer role prefix and no fallback brackets.
This operation is useful for tests and for callers that already split operands.

`PeerUrlParser.normalize_identity(text, context) -> String`

Parses one URL candidate and returns only its canonical identity.

## Parsing Semantics

Supported candidate forms:

| Form | Meaning |
| --- | --- |
| `/path`, `./relative`, `../relative`, `c:\path`, `c:/path` | Bare local path, equivalent to `file://`. |
| `file:///path` | Local filesystem URL. |
| `sftp://user@host/path` | SFTP URL with username and default port `22`. |
| `sftp://host/path` | SFTP URL with `current_os_user` inserted as the username. |
| `sftp://user:password@host/path` | SFTP URL with inline password. |
| `sftp://user@host:port/path` | SFTP URL with explicit SSH port. |

Fallback groups use comma separation at the top bracket level. A comma needed
inside a candidate URL must be percent-encoded by the caller. Empty fallback
groups and empty candidates are invalid. Nested fallback groups are invalid.

Query parameters are parsed before canonicalization. Supported parameters are
`mc`, `ct`, and `ka`; each value must be a positive base-10 integer. Query
parameters are not part of `canonical_identity`.

Percent decoding follows URL rules:

- Percent-encoded unreserved characters are decoded in canonical identities.
- Percent-encoded reserved characters remain encoded in canonical identities so
  the URL structure is not changed.
- Inline passwords are decoded for the `password` field.
- Invalid percent escapes are errors.

## Canonical Identity

Canonical identities are deterministic across operating systems and process
runs for the same `ParseContext`.

All canonical identities:

- lowercase the URL scheme;
- collapse consecutive `/` characters in the path;
- remove a trailing path slash unless the path is the root;
- strip query strings and fragments;
- decode percent-encoded unreserved characters;
- exclude inline passwords.

`file` identities:

- use the `file://` scheme;
- resolve relative paths against `current_working_directory` lexically;
- convert `\` to `/`;
- preserve path spelling except for separator conversion, lexical resolution,
  repeated slash collapse, and trailing slash removal.

`sftp` identities:

- lowercase the hostname;
- normalize omitted or default ports to `22`;
- omit `:22` from the identity;
- insert `current_os_user` when the URL has no username;
- include the username, host, non-default port when present, and absolute remote
  path.

Examples:

| Input | Context | Canonical identity |
| --- | --- | --- |
| `./data` | cwd `/home/ace/work` | `file:///home/ace/work/data` |
| `c:\photos\` | any cwd | `file:///c:/photos` |
| `SFTP://Host:22//docs/?mc=5` | user `ace` | `sftp://ace@host/docs` |
| `sftp://bilbo:p%40ss@example.com:2222/photos` | user `ace` | `sftp://bilbo@example.com:2222/photos` |

## Observable Behavior

- Candidate order is preserved exactly for fallback groups.
- Parsing is pure and performs no filesystem, network, environment, or clock
  I/O.
- Relative path resolution uses only `ParseContext.current_working_directory`;
  it does not resolve symlinks or require paths to exist.
- SFTP paths are absolute remote paths. A missing path or a relative SFTP path
  is invalid.
- `sftp://host/path` and `sftp://current_user@host:22/path` have the same
  canonical identity when `current_os_user` is `current_user`.
- `sftp://user:one@host/path` and `sftp://user:two@host/path` have the same
  canonical identity but different `password` fields.
- Public operations do not print to stdout or stderr.

## Error Behavior

Invalid inputs fail with one of these categories and no partial public result:

| Category | Meaning |
| --- | --- |
| `empty_operand` | The peer operand or URL candidate is empty. |
| `invalid_role_prefix` | A role prefix appears inside a fallback group or more than one role prefix appears. |
| `invalid_fallback_group` | Brackets are unbalanced, nested, empty, or contain an empty candidate. |
| `unsupported_scheme` | The candidate has a scheme other than `file` or `sftp`. |
| `invalid_file_url` | A `file://` URL has unsupported authority, user info, port, or invalid path syntax. |
| `invalid_sftp_url` | An SFTP URL lacks a host, has an invalid port, has no absolute path, or has invalid user-info syntax. |
| `invalid_setting` | A query parameter is not `mc`, `ct`, or `ka`, appears more than once, or has a non-positive or non-integer value. |
| `invalid_percent_encoding` | A percent escape is incomplete or contains non-hex characters. |
| `invalid_context` | The context has an empty current user or a non-absolute current working directory. |

The library must not throw transport-specific, filesystem-specific,
authentication, database, or sync-decision errors because it performs none of
those operations.

## Examples

### Canon Peer With Fallback URLs

Input:

```text
[
  sftp://Host:22//photos/?mc=5&ct=60,
  sftp://bilbo:p%40ss@backup.example:2222/photos
]

context:
  current_working_directory = /home/ace/work
  current_os_user = ace
```

Written as one operand:

```text
+[sftp://Host:22//photos/?mc=5&ct=60,sftp://bilbo:p%40ss@backup.example:2222/photos]
```

Output:

```text
role = canon
candidates[0] =
  scheme = sftp
  canonical_identity = sftp://ace@host/photos
  user = ace
  password = absent
  host = host
  port = 22
  path = /photos
  endpoint_key = ace@host:22
  settings = { max_connections = 5, connect_timeout_seconds = 60 }

candidates[1] =
  scheme = sftp
  canonical_identity = sftp://bilbo@backup.example:2222/photos
  user = bilbo
  password = p@ss
  host = backup.example
  port = 2222
  path = /photos
  endpoint_key = bilbo@backup.example:2222
  settings = {}
```

### Bare Local Path

Input:

```text
text = ./data/
context.current_working_directory = /home/ace/work
```

Output:

```text
role = normal
candidates = [
  scheme = file
  canonical_identity = file:///home/ace/work/data
  path = /home/ace/work/data
  settings = {}
]
```

### Windows Drive Path

Input:

```text
text = -c:\photos\raw
context.current_working_directory = /home/ace/work
```

Output:

```text
role = subordinate
candidates = [
  scheme = file
  canonical_identity = file:///c:/photos/raw
  path = c:/photos/raw
  settings = {}
]
```

## Testing Requirements

Tests are black-box tests of the public API. No external service account, SFTP
server, SSH key, known-hosts file, SQLite database, local filesystem fixture,
or network access is required. Tests must supply `ParseContext` explicitly
instead of reading the process environment.

Required scenarios:

- Bare POSIX-style absolute paths, relative paths, and Windows drive paths are
  parsed as `file` candidates.
- Relative local paths resolve lexically against the supplied current working
  directory without requiring files to exist.
- `file://` and bare-path inputs produce the specified canonical `file://`
  identities.
- `sftp://` inputs parse usernames, inline passwords, hosts, default and
  explicit ports, and absolute paths.
- Missing SFTP usernames are filled from `current_os_user`.
- Hostnames and schemes are lowercased, default SFTP port `22` is omitted from
  canonical identities, repeated path slashes collapse, trailing slashes are
  removed, and query strings are stripped from identities.
- `mc`, `ct`, and `ka` query settings parse as positive integers and remain
  associated with the candidate that declared them.
- Fallback groups preserve candidate order, reject empty candidates, and apply
  a leading role prefix to the whole group.
- Prefixes `+`, `-`, and no prefix produce `canon`, `subordinate`, and `normal`
  roles.
- Passwords and query settings do not change `canonical_identity`.
- Invalid percent escapes, unsupported schemes, malformed fallback brackets,
  role prefixes inside fallback groups, invalid SFTP ports, relative SFTP paths,
  duplicate or unknown query settings, and invalid context values report the
  specified errors.
- No public operation emits stdout or stderr.

Scenarios to avoid:

- Do not test network reachability, SSH authentication, host-key verification,
  SFTP sessions, or connection pooling behavior.
- Do not test local filesystem existence, directory creation, symlink
  resolution, file metadata, or case sensitivity of real filesystems.
- Do not test global command-line option parsing, help text formatting,
  validation of the number of peer operands, or the rule that a full invocation
  may contain at most one canon peer.
- Do not test sync conflict decisions, traversal, ignore-file resolution,
  snapshots, tombstones, BAK/TMP path creation, file copies, or cleanup.

## Semantic Anchors

This specification is anchored in:

- RFC 3986, Uniform Resource Identifier syntax
- RFC 8089, The `file` URI Scheme
- The semantic source sections for peer operands, role prefixes, fallback URLs,
  per-URL settings, supported URL schemes, inline SFTP passwords, current-user
  insertion for SFTP URLs, and URL normalization
