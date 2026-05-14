# Private Key File Authentication

## Purpose
Authenticate an SSH transport as `user` using default private key files for SSH public key authentication.

## Public API
Data shapes:

- `SshTransport`: SSH transport after a successful SSH handshake.
- `PrivateKeyFile`: `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, or `~/.ssh/id_rsa`.
- `AuthenticatedSshSession`: SSH session authenticated as `user`.

Operations:

- `authenticate_private_key_files(transport, user) -> AuthenticatedSshSession | unavailable | rejected`: try default `PrivateKeyFile` credential sources for SSH public key authentication.

## Behavior
`authenticate_private_key_files` tries private key files in this order:

1. `~/.ssh/id_ed25519`
2. `~/.ssh/id_ecdsa`
3. `~/.ssh/id_rsa`

The first successful SSH public key authentication returns an `AuthenticatedSshSession`.

Unreadable private key files are skipped as unavailable credential sources.

Malformed private key files are counted as rejected credential sources and the next private key file is tried.

If no private key file succeeds, the operation returns `rejected` when any private key file was malformed or rejected by the SSH server; otherwise it returns `unavailable`.

## Errors
SSH transport failure during authentication is reported as `io_error`.

Unreadable private key files are treated as unavailable credential sources.

Malformed private key files are treated as rejected credential sources.

## Anchoring
`user`, private key files, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, `~/.ssh/id_rsa`, unreadable private key files, malformed private key files, `unavailable`, and `rejected` are anchored in `sync.md` "URL Schemes" and "Authentication".

`SshTransport` and SSH transport failure are anchored in RFC 4253.

SSH user authentication and public key authentication are anchored in RFC 4252.

`AuthenticatedSshSession` is anchored in RFC 4252 authentication success semantics and RFC 4254 SSH connection behavior.

`io_error` is anchored in `sync.md` "Peer Transports" / "Error Semantics".
