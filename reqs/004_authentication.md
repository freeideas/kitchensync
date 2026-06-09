# 004_authentication: SFTP authentication and host verification

## Behavior
This concern derives from `specs/sync.md` sections "URL Schemes" (password
percent-encoding) and "Authentication (fallback chain)".

It covers how an SFTP connection authenticates: the ordered credential fallback
chain (inline URL password, then SSH agent via `SSH_AUTH_SOCK`, then
`~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`), the
requirement that each source is attempted in this exact order and a missing or
rejected source falls through to the next, host-key verification through
`~/.ssh/known_hosts` with unknown hosts rejected, and percent-decoding of
special characters in inline SFTP passwords.

The act of selecting and connecting a winning URL is
`005_connection-establishment`. SFTP timeout settings are exercised under
`005_connection-establishment` and `020_copy-execution`.

## $REQ_IDs

- `004.1` -- The inline URL password is the first credential source attempted for an SFTP connection.
- `004.2` -- The SSH agent identified by `SSH_AUTH_SOCK` is the second credential source attempted.
- `004.3` -- `~/.ssh/id_ed25519` is the third credential source attempted.
- `004.4` -- `~/.ssh/id_ecdsa` is the fourth credential source attempted.
- `004.5` -- `~/.ssh/id_rsa` is the fifth credential source attempted.
- `004.6` -- A credential source that is absent is skipped and the next source in the order is attempted.
- `004.7` -- A credential source rejected by the host causes the next source in the order to be attempted.
- `004.8` -- A host whose key matches its `~/.ssh/known_hosts` entry passes host-key verification.
- `004.9` -- A host absent from `~/.ssh/known_hosts` is rejected.
- `004.10` -- An inline SFTP password containing percent-encoded characters is percent-decoded before authentication (`%40` becomes `@`, `%3A` becomes `:`).
