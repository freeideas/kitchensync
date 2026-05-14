# Known Hosts Verification

## Purpose
Verify an SSH server host key against `~/.ssh/known_hosts` and reject unknown hosts.

## Public API
Data shapes:

- `KnownHostsPath`: filesystem path to an OpenSSH `known_hosts` file
- `SshHost`: `host`, `port`
- `HostKey`: SSH public host key from the SSH handshake
- `HostKeyVerification`: `accepted` or `rejected`

Operations:

- `verify_host_key(known_hosts_path, ssh_host, host_key) -> HostKeyVerification`: verify the SSH handshake host key for an SSH host.

## Behavior
`verify_host_key` reads host key entries from `known_hosts_path`.

A host key is accepted only when the SSH host matches a `known_hosts` entry for `host` and `port` and the received host key matches that entry.

A host key is rejected when no matching host entry exists or the matching host entry has a different host key.

## Errors
Invalid or unreadable `known_hosts_path` fails verification.

Malformed `known_hosts` entries that prevent verification fail verification.

Host key mismatch fails verification.

Unknown host fails verification.

## Anchoring
`~/.ssh/known_hosts`, host key verification, unknown host rejection, SSH handshake, `host`, and `port` are anchored in `SFTP Connection Establishment`.

`HostKey` and SSH public host key semantics are anchored in RFC 4253.

`known_hosts` entry matching is anchored in the OpenSSH `known_hosts` file format.
