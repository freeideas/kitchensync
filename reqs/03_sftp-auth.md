# 03_sftp-auth: SFTP authentication and host key checking

## Behavior

To authenticate an SFTP connection, the program tries each available method in a fixed order until one succeeds. Host keys for SSH peers are verified against `~/.ssh/known_hosts`; unknown hosts are rejected rather than auto-trusted. Derived from `sync.md` §"Authentication (fallback chain)" and `README.md` §"Authentication".

## $REQ_IDs

- `03.65` — When an SFTP URL embeds a password (`sftp://user:password@host/path`), that password is tried first.
- `03.66` — If no inline password is present (or it fails), the program tries the SSH agent via `SSH_AUTH_SOCK`.
- `03.67` — If agent authentication is not available (or fails), the program tries identity files in order: `~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`.
- `03.68` — When connecting to an SFTP host, the host key is verified against `~/.ssh/known_hosts`.
- `03.69` — An SFTP host whose key is not in `~/.ssh/known_hosts` is rejected (the URL is treated as a failed connection).
- `03.70` — Percent-encoded characters in inline SFTP passwords are decoded before authentication (e.g. `%40` → `@`).

## Notes

SFTP connection failures during a run surface as transfer failures — see `04_error-handling.md`.
