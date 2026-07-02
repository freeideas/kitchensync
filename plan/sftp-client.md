# SFTP Client

## Risk

The specs require SFTP peers with host-key verification through
`known_hosts`, password authentication, SSH saved-key fallback including
Ed25519, and transport operations for listing, stat, streaming read/write,
rename-to-new-path, delete, directory create/delete, and modification time
updates. Rust standard library does not provide this.

## Experiment

`plan/experiments/sftp-client` starts `extart/ephemeral-sftp-server.py` twice:

- once with `--user alice --password secret`;
- once with `--user alice --authorized-key id_ed25519.pub`.

The client uses `ssh2` `0.9.5` and asserts:

- `Session::new`, `set_tcp_stream`, and `handshake` connect to the local server;
- `Session::host_key`, `Session::known_hosts`,
  `KnownHosts::read_str`, and `KnownHosts::check_port` verify the printed host
  key. `CheckResult` does not implement `PartialEq`, so code must use
  `matches!(..., CheckResult::Match)` or equivalent matching;
- `Session::userauth_password` succeeds for the password server;
- `Session::userauth_pubkey_file` succeeds with an OpenSSH Ed25519 private key
  when the server is key-only;
- `Session::sftp` returns an SFTP handle;
- `Sftp::mkdir`, `Sftp::create`, `Write::write_all`, `Sftp::stat`,
  `Sftp::setstat`, `Sftp::rename`, `Sftp::open`, `Read::read_to_string`,
  `Sftp::readdir`, `Sftp::unlink`, and `Sftp::rmdir` work against the fixture.

## Proven Package

- `ssh2` `0.9.5`

## Notes For Later Code

Use `FileStat { mtime: Some(seconds), atime: Some(seconds), .. }` through
`Sftp::setstat` for SFTP modification time updates. The experiment proves
second-level SFTP mtime through this fixture, not subsecond precision.

The experiment only renames to a missing destination. The specs already require
SWAP replacement because some SFTP servers reject rename over an existing
destination, so product code must not depend on overwrite behavior.

