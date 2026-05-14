# Known Hosts Entry Matching

## Purpose
Read an OpenSSH `known_hosts` file and locate SSH public host keys for a host and port.

## Public API
Data shapes:

- `KnownHostsPath`: filesystem path to an OpenSSH `known_hosts` file
- `SshHost`: `host`, `port`
- `HostKey`: SSH public host key from a `known_hosts` entry
- `KnownHostsMatch`: `matched(host_keys)`, `not_found`, or `failed`

Operations:

- `match_known_hosts_entries(known_hosts_path, ssh_host) -> KnownHostsMatch`: read `known_hosts_path` and return host keys from entries matching `host` and `port`.

## Behavior
`match_known_hosts_entries` reads host key entries from `known_hosts_path`.

A `known_hosts` entry matches only when its host pattern matches `host` and `port` according to the OpenSSH `known_hosts` file format.

When one or more entries match, the operation returns `matched(host_keys)` containing the SSH public host keys from those entries.

When no entry matches, the operation returns `not_found`.

## Errors
Invalid or unreadable `known_hosts_path` returns `failed`.

Malformed `known_hosts` entries that prevent matching return `failed`.

## Anchoring
`KnownHostsPath`, `SshHost`, `host`, `port`, `~/.ssh/known_hosts`, and unknown host rejection are anchored in `Known Hosts Verification`.

`HostKey` and SSH public host key semantics are anchored in RFC 4253.

`known_hosts` entry matching is anchored in the OpenSSH `known_hosts` file format.
