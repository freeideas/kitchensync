# Testing Guidelines

Tests that need a real SFTP endpoint use the bundled ephemeral server rather than
any external host, so they run offline. Do not connect to external SFTP hosts;
point tests at `127.0.0.1`.

## The ephemeral SFTP server

`extart/ephemeral-sftp-server.py` is a self-contained SFTP server for tests. Launch
it as a subprocess through the bundled uv for the platform:

```
aisf/bin/uv.linux  run --script extart/ephemeral-sftp-server.py   # Linux
aisf/bin/uv.mac    run --script extart/ephemeral-sftp-server.py   # macOS
aisf/bin/uv.exe    run --script extart/ephemeral-sftp-server.py   # Windows
```

- It binds `127.0.0.1` on an OS-assigned port and prints **exactly one line to
  stdout: that port number**. Read that line, then connect the SFTP peer to
  `127.0.0.1:<port>`. (The temp root, host public key, and auth mode go to
  stderr.)
- Everything written through the server lives in a temporary directory that is
  **deleted when the process stops**. Stand the server up in test setup and
  **terminate the process in teardown**; uploaded files disappear with it.
- Authentication -- compose these flags (passed after the script path):
  - `--user NAME`: require this username (default: any username);
  - `--password PW`: accept password `PW` (default: any password);
  - `--authorized-key FILE`: accept the OpenSSH public key in `FILE`. With a key
    and no `--password` the server is **key-only** (rejects passwords) -- the
    saved-key / Ed25519 case; with both, it accepts **either** (fallback testing);
  - `--host-key FILE`: present a fixed private host key (Ed25519/ECDSA/RSA)
    instead of a fresh one, so a test can pin it.
- The server prints its host public key to stderr as `host key: <type> <base64>`,
  so a test that exercises host-key verification can build a `known_hosts` entry:
  `[127.0.0.1]:<port> <type> <base64>`.

## Symlinks

KitchenSync silently omits symbolic links and special files from listings (see
multi-tree-sync.md, Built-in Excludes). Tests must not create symlinks, read
symlink targets, request symlink following, or require symlink-specific behavior.
It is acceptable to assert that an existing symlink is ignored only when one
occurs naturally; do not add setup code that depends on creating one.

## Required authentication coverage

Authentication coverage must include a case where the server accepts only an
Ed25519 public key corresponding to `~/.ssh/id_ed25519`, rejects or lacks an RSA
key, and the client has no inline password and no usable SSH agent. Start the
server with `--authorized-key` pointing at that Ed25519 public key. That test must
prove KitchenSync can connect through the documented fallback chain without
depending on `id_rsa`.
