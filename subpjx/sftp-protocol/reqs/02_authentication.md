# 02_authentication: SSH authentication method order

## Behavior

When opening a new SSH+SFTP session, authentication methods are tried in a fixed order and the first that succeeds wins. The order is: inline URL password, SSH agent (via `SSH_AUTH_SOCK`), `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, `~/.ssh/id_rsa`. Derives from `specs/SPEC.md` § "API surface > Authentication".

## $REQ_IDs

- `02.10` — When the URL contains an inline password and the server accepts it, the connection authenticates using that password.
- `02.11` — When no inline password is present (or it is rejected), authentication is attempted via the SSH agent reached through `SSH_AUTH_SOCK`.
- `02.12` — When prior methods do not succeed, `~/.ssh/id_ed25519` is attempted.
- `02.13` — When prior methods do not succeed, `~/.ssh/id_ecdsa` is attempted after `id_ed25519`.
- `02.14` — When prior methods do not succeed, `~/.ssh/id_rsa` is attempted after `id_ecdsa`.
- `02.15` — Authentication stops at the first method that succeeds; later methods in the order are not tried.

## Notes

- Failure of all authentication methods surfaces as an I/O error — see [[03_error-categories]].
