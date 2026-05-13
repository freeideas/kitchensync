# 02_host-key-verification: Server host key verification against known_hosts

## Behavior

Server host keys are verified against the user's `~/.ssh/known_hosts` file (OpenSSH format). A connection to a host whose key is not recorded, or whose key does not match the recorded entry, is rejected as a connection failure. Derives from `specs/SPEC.md` § "API surface > Host key verification".

## $REQ_IDs

- `02.16` — A connection to a host with a matching entry in `~/.ssh/known_hosts` is accepted (verification succeeds).
- `02.17` — A connection to a host that has no entry in `~/.ssh/known_hosts` is rejected as a connection failure.
- `02.18` — A connection to a host whose presented key does not match the entry in `~/.ssh/known_hosts` is rejected as a connection failure.

## Notes

- Connection failures surface as I/O errors — see [[03_error-categories]].
