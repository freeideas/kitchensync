# SSH Authentication Chain

## Purpose
Authenticate an SSH transport for an `sftp://` URL by trying the required credential sources in order.

## Public API
Data shapes:

- `SshTransport`: SSH transport after a successful SSH handshake
- `SftpCredentials`: `user`, `password` optional
- `AuthenticatedSshSession`: SSH session authenticated as `user`
- `AuthenticationAttempt`: `inline_password`, `ssh_agent`, or `private_key_file`

Operations:

- `authenticate_ssh(transport, credentials) -> AuthenticatedSshSession`: authenticate `credentials.user` on `transport` using the SFTP authentication fallback chain.

## Behavior
`authenticate_ssh` tries authentication methods in this order:

1. `credentials.password`, when present
2. SSH agent from `SSH_AUTH_SOCK`
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_ecdsa`
5. `~/.ssh/id_rsa`

The first successful authentication returns an `AuthenticatedSshSession`.

If an authentication method is unavailable, `authenticate_ssh` skips it and tries the next method.

If an authentication method is available but rejected by the SSH server, `authenticate_ssh` tries the next method.

## Errors
Authentication fails if every available authentication method is rejected or unavailable.

SSH transport failure during authentication is reported as `io_error`.

Unreadable private key files are treated as unavailable credential sources.

Malformed private key files are treated as rejected credential sources.

## Anchoring
`user`, `password`, `sftp://`, inline password, SSH agent, `SSH_AUTH_SOCK`, and private key fallback order are anchored in `sync.md` "URL Schemes" and "Authentication".

`SshTransport` and SSH transport failure are anchored in RFC 4253.

SSH user authentication, password authentication, and public key authentication are anchored in RFC 4252.

`AuthenticatedSshSession` is anchored in RFC 4252 authentication success semantics and RFC 4254 SSH connection behavior.

`io_error` is anchored in `sync.md` "Peer Transports" / "Error Semantics".
