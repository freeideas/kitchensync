# Testing Guidelines

Tests that need SFTP should spin up an in-process SFTP server on `127.0.0.1`
with a random port (paramiko provides server-side SFTP support). Stand the
server up in setup, tear it down in teardown. Do not depend on external SFTP
hosts; tests must run offline.

SFTP authentication coverage must include a case where the server accepts only
an Ed25519 public key corresponding to `~/.ssh/id_ed25519`, rejects or lacks an
RSA key, and the client has no inline password and no usable SSH agent. That
test must prove KitchenSync can connect through the documented fallback chain
without depending on `id_rsa`.
