# 03_authentication: SSH auth fallback chain and host-key verification

## Behavior

When opening a fresh SSH connection to an endpoint, authentication is attempted in a fixed order — inline password (if supplied), then SSH agent, then `~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa` — stopping at the first method that succeeds. Independently, the server's host key is verified against `~/.ssh/known_hosts` on every fresh connection; an unknown host causes the connection attempt to fail as an I/O error. Derived from `SPEC.md` §"Authentication" (and the host-key sentence in §"Acquiring and releasing pooled connections").

## $REQ_IDs
- `03.1` — When an inline password is supplied at `open_endpoint`, authentication tries that password first.
- `03.2` — If no inline password is supplied or it is rejected, authentication next tries the SSH agent at `$SSH_AUTH_SOCK`.
- `03.3` — If the SSH agent is unavailable or its keys are rejected, authentication next tries `~/.ssh/id_ed25519`.
- `03.4` — If `~/.ssh/id_ed25519` is unavailable or rejected, authentication next tries `~/.ssh/id_ecdsa`.
- `03.5` — If `~/.ssh/id_ecdsa` is unavailable or rejected, authentication next tries `~/.ssh/id_rsa`.
- `03.6` — Authentication stops at the first method in the order above that succeeds, and the connection proceeds.
- `03.7` — A host whose key is not present in `~/.ssh/known_hosts` (or does not match an entry there) causes the connection attempt to fail as an I/O error.
