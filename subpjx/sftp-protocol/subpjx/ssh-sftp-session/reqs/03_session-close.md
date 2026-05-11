# 03_session-close: close_session shuts down the SFTP subsystem and SSH transport.

## Behavior
`close_session` (specs/SPEC.md §"Closing") closes the SFTP subsystem and the underlying SSH transport for a session that was previously opened via `open_session`. The spec requires that any operations in flight against the session must complete (or fail) before the session is closed — close does not race with outstanding operations.

## $REQ_IDs
- `03.1` — `close_session` on an open session shuts it down without error.
- `03.2` — An operation issued before `close_session` returns either a normal result or a failure (not a torn/partial outcome) before `close_session` returns.

## Notes
03.2 is the spec's "in-flight operations must complete (or fail) before the session is closed" guarantee. Observable test shape: issue an operation, immediately call `close_session`, and verify the operation reported a defined success-or-failure result.
