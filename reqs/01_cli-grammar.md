# 01_cli-grammar: Default values for global options

## Behavior

Each global option (`--mc`, `--ct`, `--ka`, `-vl`, `--xd`, `--bd`, `--td`) has a documented default value that takes effect when the corresponding flag is not given on the command line. Derived from `sync.md` §"Global Options" and `specs/README.md` §"Global Options".

## $REQ_IDs

- `01.24` — When `--mc` is omitted, max SFTP connections defaults to 10.
- `01.29` — When `--ct` is omitted, SSH handshake timeout defaults to 30 seconds.
- `01.30` — When `--ka` is omitted, SFTP idle keep-alive TTL defaults to 30 seconds.
- `01.31` — When `-vl` is omitted, verbosity defaults to `info`.
- `01.32` — When `--xd` is omitted, stale TMP staging cleanup defaults to 2 days.
- `01.33` — When `--bd` is omitted, BAK cleanup defaults to 90 days.
- `01.34` — When `--td` is omitted, deletion-record cleanup defaults to 180 days.

## Notes

What each flag does at runtime, and how per-URL query-string settings override it, is in the corresponding feature req: `--mc`/`--ct`/`--ka` in `03_sftp-pool.md`, `-vl` in `03_logging.md`, and `--xd`/`--bd`/`--td` in `04_retention.md`. Accepted URL forms (`sftp://`, bare paths) and `+`/`-`/`[...]` peer prefixes are exercised by the reqs whose behaviors require them: `03_sftp-auth.md`, `03_sftp-pool.md`, `03_canon-peer.md`, `03_subordinate-peer.md`, and `03_fallback-urls.md`. Bare-path → `file://` normalization is in `02_url-normalization.md`. Validation of invalid forms or values is in `01_cli-validation.md`.
