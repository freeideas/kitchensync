# URL Normalization

## Risk

KitchenSync needs to parse SFTP URLs, strip default port 22, strip query-string
settings from identity, insert the current OS user when no SFTP username is
present, decode percent-encoded passwords, and create file URLs for local paths.
The Rust standard library does not provide URL parsing.

## Experiment

`plan/experiments/url-normalization` uses:

- `url` `2.5.4`;
- `percent-encoding` `2.3.1`;
- `whoami` `1.5.2`.

It asserts:

- `Url::parse("SFTP://Host:22//docs/%7Ealice?timeout-conn=60")` lowercases the
  scheme to `sftp`, keeps port `22`, preserves the query pair, and keeps the
  path string;
- for the non-special `sftp` scheme, `Url::host_str()` preserves `Host` case.
  Product code must call `to_ascii_lowercase()` itself for URL identity;
- `Url::set_port(None)` removes default port 22;
- `Url::set_query(None)` strips per-URL settings from identity;
- repeated slashes in the path can be collapsed by product code after parsing;
- `percent_decode_str(...).decode_utf8()` decodes `%7E` in paths and
  `%40`/`%3A` in passwords;
- `Url::set_username(&whoami::username())` inserts a current OS user;
- `Url::from_directory_path` creates `file://` URLs for absolute local paths.

## Proven Packages

- `url` `2.5.4`
- `percent-encoding` `2.3.1`
- `whoami` `1.5.2`

## Notes For Later Code

Do not assume `url` fully normalizes SFTP identity. Lowercase the host,
collapse path slashes, remove trailing slashes, strip the query, and remove
default port 22 explicitly.

