# SSH Transport Handshake

## Purpose
Open an SSH transport to a network endpoint and complete the SSH handshake within the configured connection timeout.

## Public API
Data shapes:

- `SshEndpoint`: `host`, `port`
- `ConnectionTimeout`: `connection_timeout_seconds`
- `HostKey`: SSH public host key received during the SSH handshake
- `SshTransport`: SSH transport after a successful SSH handshake
- `HandshakeResult`: `transport`, `host_key`

Operations:

- `open_ssh_transport(endpoint, timeout) -> HandshakeResult`: connect to `endpoint` and complete the SSH handshake before `timeout.connection_timeout_seconds` elapses.

## Behavior
`open_ssh_transport` opens a network connection to `endpoint.host` and `endpoint.port`.

The SSH handshake is bounded by `connection_timeout_seconds`.

The operation completes SSH transport protocol version exchange, algorithm negotiation, key exchange, and server host key receipt.

On success, `open_ssh_transport` returns a usable `SshTransport` and the received `HostKey`.

## Errors
Handshake establishment fails if `connection_timeout_seconds` elapses before the SSH handshake completes.

Handshake establishment fails if the endpoint cannot be reached or the SSH transport handshake cannot be completed.

Network drop or SSH transport I/O failure during handshake is reported as `io_error`.

## Anchoring
`host`, `port`, and SSH endpoint addressing are anchored in `sync.md` "URL Schemes".

`connection_timeout_seconds` is anchored in `concurrency.md` "Connection Establishment".

`SshTransport`, SSH transport protocol version exchange, algorithm negotiation, key exchange, and `HostKey` are anchored in RFC 4253.

`io_error` is anchored in `sync.md` "Peer Transports" / "Error Semantics".
