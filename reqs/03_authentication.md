# 03_authentication: SFTP authentication chain

## Behavior

For SFTP peers, KitchenSync tries an ordered chain of authentication methods and verifies host keys against `~/.ssh/known_hosts`. Derived from `specs/sync.md` §"Authentication" and `specs/README.md` §"Authentication".

## $REQ_IDs
- `03.39` — When an SFTP URL carries an inline password, that password is the first authentication method tried for that URL.
- `03.40` — If no inline password is present (or it fails), the SSH agent at `$SSH_AUTH_SOCK` is tried next.
- `03.41` — If the SSH agent fails or is unavailable, identity files are tried in order: `~/.ssh/id_ed25519`, then `~/.ssh/id_ecdsa`, then `~/.ssh/id_rsa`.
- `03.42` — An SFTP connection whose host key matches an entry in `~/.ssh/known_hosts` is accepted.
- `03.53` — An SFTP connection to a host whose key is not present in `~/.ssh/known_hosts` is rejected and the connection fails.
