# Testing Guidelines

Tests that need SFTP should spin up an in-process SFTP server on `127.0.0.1`
with a random port (paramiko provides server-side SFTP support). Stand the
server up in setup, tear it down in teardown. Do not depend on external SFTP
hosts; tests must run offline.
