# 02_scheme-predicates: is_file_url and is_sftp_url scheme dispatchers

## Behavior

`is_file_url` and `is_sftp_url` are scheme-dispatch predicates that callers use to pick a transport for a parsed `Url`. Each returns true exactly when the `Url`'s scheme matches its name. Derived from `SPEC.md` §"Convenience".

## $REQ_IDs
- `02.30` — `is_file_url` returns true for a `Url` whose scheme is `file`.
- `02.31` — `is_file_url` returns false for a `Url` whose scheme is `sftp`.
- `02.32` — `is_sftp_url` returns true for a `Url` whose scheme is `sftp`.
- `02.33` — `is_sftp_url` returns false for a `Url` whose scheme is `file`.
