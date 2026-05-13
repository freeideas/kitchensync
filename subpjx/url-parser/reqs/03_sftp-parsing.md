# 03_sftp-parsing: SFTP URL field population and validation

## Behavior

An `sftp://` URL follows the RFC 3986 generic-URI grammar with optional userinfo (`user` or `user:password`), a required host, an optional port, and an absolute path. The parser populates the corresponding fields on `ParsedUrl` and rejects sftp URLs that lack a host or carry a port outside the valid TCP range. Derived from SPEC.md section "Grammar" (sftp bullet), "Output structure", and the rejection list in "API surface".

## $REQ_IDs

- `03.1` — An sftp URL of the form `sftp://user@host/path` populates `ParsedUrl.user` with the given user.
- `03.2` — An sftp URL of the form `sftp://user:password@host/path` populates `ParsedUrl.user` and `ParsedUrl.password`.
- `03.3` — `ParsedUrl.host` is populated from the URL's authority for sftp URLs.
- `03.4` — An sftp URL with an explicit port populates `ParsedUrl.port` with that integer.
- `03.5` — An sftp URL without a host is rejected.
- `03.6` — An sftp URL with a port outside `1..=65535` is rejected.

## Notes

`ParsedUrl.user`, `password`, `host`, and `port` are sftp-only fields. The handling of a missing userinfo for identity computation (insertion of `default_user`) is covered in [03_identity-normalization](03_identity-normalization.md).
