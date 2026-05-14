# SFTP Connection Establishment

## Purpose
Establish an SSH+SFTP session for an `sftp://` peer, including SSH handshake, SSH authentication, host key verification, and peer root-path creation.

## Public API
Data shapes:

- `SftpPeer`: `user`, `password` optional, `host`, `port`, `root_path`
- `PoolSettings`: `connection_timeout_seconds` default `30`
- `Session`: established SSH+SFTP session with a peer root path

Operations:

- `connect_listing(peer, settings) -> Session`: open an unpooled SSH+SFTP session.

## Behavior
`connect_listing` performs an SSH handshake bounded by `connection_timeout_seconds`.

Authentication uses inline password first, then SSH agent, then `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, and `~/.ssh/id_rsa`.

Host keys are verified through `~/.ssh/known_hosts`; unknown hosts are rejected.

After SSH+SFTP connection succeeds, the peer root path is checked. If it does not exist, it and any missing parents are created through SFTP before the session is returned.

## Errors
Connection establishment fails if the handshake times out, authentication fails, the host key is rejected, the root path cannot be created, or an SSH/SFTP I/O failure occurs.

Network drop, SSH channel failure, timeout after connection, and SFTP protocol failure are reported as `io_error`.

## Anchoring
`SftpPeer`, SSH handshake, host key verification, and authentication order are anchored in `sync.md` "URL Schemes" and "Authentication".

`PoolSettings.connection_timeout_seconds` and `connect_listing` are anchored in `concurrency.md` "Connection Establishment" and "Directory Listing".

`Session` and SSH transport/session behavior are anchored in RFC 4253 and RFC 4254.

SFTP root-path creation is anchored in SFTP filesystem semantics from `draft-ietf-secsh-filexfer`.

Error categories are anchored in `sync.md` "Peer Transports" / "Error Semantics".
