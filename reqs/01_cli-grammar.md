# 01_cli-grammar: Default values for global options

## Behavior

Each global option (`--mc`, `--ct`, `--ka`, `-vl`, `--xd`, `--bd`, `--td`) has a documented default value that takes effect when the corresponding flag is not given on the command line. Derived from `sync.md` §"Global Options".

## $REQ_IDs

- `01.24` — When no flag is given, the defaults are: `--mc 10`, `--ct 30`, `--ka 30`, `-vl info`, `--xd 2`, `--bd 90`, `--td 180`.

## Notes

What each flag does at runtime, and how per-URL query-string settings override it, is in the corresponding feature req: `--mc`/`--ct`/`--ka` in `03_sftp-pool.md`, `-vl` in `03_logging.md`, and `--xd`/`--bd`/`--td` in `04_retention.md`. Accepted URL forms (`sftp://`, bare paths) and `+`/`-`/`[...]` peer prefixes are exercised by the reqs whose behaviors require them: `03_sftp-auth.md`, `03_sftp-pool.md`, `03_canon-peer.md`, `03_subordinate-peer.md`, and `03_fallback-urls.md`. Bare-path → `file://` normalization is in `02_url-normalization.md`. Validation of invalid forms or values is in `01_cli-validation.md`.
