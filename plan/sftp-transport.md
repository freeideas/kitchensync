# SFTP Transport

## Risk

The specs require SFTP peers with host-key verification, the documented
authentication fallback chain including saved Ed25519 keys, and file operations
that match the local transport surface.

## Experiment

`experiments/sftp-transport` is a Rust mini-project using:

- `ssh2` `0.9.5`

It launches `extart/ephemeral-sftp-server.py` through
`aisf/bin/uv.linux run --script`, configures the server as key-only with an
Ed25519 public key, reads the server port and host public key, then connects to
`127.0.0.1`.

## Proved Calls

- `Session::new`, `Session::set_tcp_stream`, `Session::set_timeout`, and
  `Session::handshake` establish the SSH session.
- `Session::host_key` returns the remote host key after handshake.
- `Session::known_hosts`, `KnownHosts::read_file`, and
  `KnownHosts::check_port` work with an OpenSSH known-hosts line of the form
  `[127.0.0.1]:<port> <type> <base64>`.
- `KnownHosts::check_port` returns `CheckResult::NotFound` before loading the
  known-hosts file and `CheckResult::Match` after loading it. `CheckResult`
  does not implement `PartialEq`; use `matches!`.
- `Session::userauth_password` fails against the key-only server and leaves the
  session unauthenticated.
- `Session::userauth_pubkey_file("plan", Some(public_key), private_key, None)`
  authenticates with an Ed25519 private key file.
- `Session::sftp` creates an SFTP handle.
- `Sftp::mkdir`, `Sftp::create`, `Sftp::open`, `Sftp::stat`,
  `Sftp::setstat`, `Sftp::readdir`, and `Sftp::rename` work against the bundled
  server.
- `Sftp::setstat` with `FileStat { atime: Some(seconds), mtime:
  Some(seconds), .. }` sets second-resolution modification time.

## Surprise

Against the bundled ephemeral SFTP server on this Linux machine,
`Sftp::rename(source, existing_destination, None)` overwrote the existing
destination. That proves the fixture does not exercise the spec's
rename-over-existing rejection case. Product code must still use the SWAP flow
from the specs and must not rely on overwrite behavior.
