# 02_session-open: open_session establishes an authenticated SSH+SFTP session.

## Behavior
`open_session` (specs/SPEC.md §"Opening a session") establishes one SSH transport to the given host and port, verifies the server's host key against `~/.ssh/known_hosts` (rejecting unknown hosts as I/O failure), authenticates the user by trying each supplied `Credential` in order until one succeeds, and starts the SFTP subsystem over the established channel. The handshake and authentication together are bounded by `connect_timeout_secs`. The three credential variants — `Password`, `Agent`, and `PrivateKeyFile` — each correspond to an RFC 4252 authentication method.

## $REQ_IDs
- `02.1` — `open_session` returns a usable session given a reachable host/port and a working credential.
- `02.2` — `open_session` returns an `io_failure` when the server's host key is not present in `~/.ssh/known_hosts`.
- `02.3` — `open_session` succeeds when any one of the supplied credentials authenticates, even if earlier credentials fail.
- `02.4` — `open_session` returns an `io_failure` when no supplied credential authenticates.
- `02.5` — `open_session` returns an `io_failure` when handshake and authentication do not complete within `connect_timeout_secs`.
- `02.6` — A `Password` credential authenticates with the user's password.
- `02.7` — A `PrivateKeyFile` credential authenticates using a key loaded from a local file (OpenSSH or PEM format).
- `02.8` — An `Agent` credential authenticates using a key offered by the SSH agent listening on the named UNIX socket.

## Notes
"Stops at first success" and "tries in order" are internal mechanism; the observable contract is (02.3) that *any* working credential yields a session and (02.4) that *no* working credential yields `io_failure`.
