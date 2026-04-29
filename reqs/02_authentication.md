# 02_authentication: SSH credential fallback chain and host-key verification

## Behavior

For SFTP peers, KitchenSync authenticates using a documented fallback chain and verifies host keys against `~/.ssh/known_hosts`. Derived from `./specs/sync.md` (`Authentication`) and `./specs/README.md` (`Authentication`).

## $REQ_IDs
- `02.32` — An `sftp://user:password@host/path` URL authenticates using the inline password.
- `02.33` — When no inline password is supplied, authentication uses `SSH_AUTH_SOCK` (SSH agent) if available, then `~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`, in that order.
- `02.34` — Connecting to a host whose key is not in `~/.ssh/known_hosts` is rejected (the URL fails to connect).
- `02.35` — Inline-password URLs that contain percent-encoded special characters (e.g., `:` as `%3A`, `@` as `%40`) authenticate using the decoded password.

## Notes

The intent is verifiable using a localhost SFTP target with a known-good key in `known_hosts` and absent/present credentials.
