# 001_cli-interface: Command-line invocation and validation

## Behavior
This concern derives from `specs/sync.md` sections "Sync", "Command Line", "Global Options", "Command-Line Excludes", and the argument-validation parts of "Startup" and "Errors", plus `specs/README.md` sections "How to run", "Why KitchenSync?", and "Exclude A Path". It covers the observable command-line tool shape, supported executable platforms, non-help invocation form, minimum peer operand count, global option names, option value validation, repeatable command-line exclude syntax and validation, unrecognized flag handling, and validation failure exit status.

## $REQ_IDs
- `001.1` -- KitchenSync is delivered as a native command-line executable named `kitchensync` for Windows, Linux, and macOS.
- `001.2` -- A non-help invocation accepts the command form `kitchensync [options] <peer> <peer> [<peer>...]`.
- `001.3` -- A non-help invocation rejects fewer than two peer operands.
- `001.4` -- A non-help invocation rejects more than one `+` peer operand.
- `001.5` -- The CLI accepts `--dry-run` as a global flag without a value.
- `001.6` -- The CLI accepts `--max-copies` with a positive integer value.
- `001.7` -- The CLI rejects a non-positive integer value for `--max-copies`.
- `001.8` -- The CLI rejects a non-integer value for `--max-copies`.
- `001.9` -- The CLI accepts `--retries-copy` with a positive integer value.
- `001.10` -- The CLI rejects a non-positive integer value for `--retries-copy`.
- `001.11` -- The CLI rejects a non-integer value for `--retries-copy`.
- `001.12` -- The CLI accepts `--retries-list` with a positive integer value.
- `001.13` -- The CLI rejects a non-positive integer value for `--retries-list`.
- `001.14` -- The CLI rejects a non-integer value for `--retries-list`.
- `001.15` -- The CLI accepts `--timeout-conn` with a positive integer value.
- `001.16` -- The CLI rejects a non-positive integer value for `--timeout-conn`.
- `001.17` -- The CLI rejects a non-integer value for `--timeout-conn`.
- `001.18` -- The CLI accepts `--timeout-idle` with a positive integer value.
- `001.19` -- The CLI rejects a non-positive integer value for `--timeout-idle`.
- `001.20` -- The CLI rejects a non-integer value for `--timeout-idle`.
- `001.21` -- The CLI accepts `--keep-tmp-days` with a positive integer value.
- `001.22` -- The CLI rejects a non-positive integer value for `--keep-tmp-days`.
- `001.23` -- The CLI rejects a non-integer value for `--keep-tmp-days`.
- `001.24` -- The CLI accepts `--keep-bak-days` with a positive integer value.
- `001.25` -- The CLI rejects a non-positive integer value for `--keep-bak-days`.
- `001.26` -- The CLI rejects a non-integer value for `--keep-bak-days`.
- `001.27` -- The CLI accepts `--keep-del-days` with a positive integer value.
- `001.28` -- The CLI rejects a non-positive integer value for `--keep-del-days`.
- `001.29` -- The CLI rejects a non-integer value for `--keep-del-days`.
- `001.30` -- The CLI accepts `--verbosity error`.
- `001.31` -- The CLI accepts `--verbosity info`.
- `001.32` -- The CLI accepts `--verbosity debug`.
- `001.33` -- The CLI accepts `--verbosity trace`.
- `001.34` -- The CLI rejects any `--verbosity` value other than `error`, `info`, `debug`, or `trace`.
- `001.35` -- The CLI rejects unrecognized flags.
- `001.36` -- The CLI accepts `-x` with a single-segment relative path value.
- `001.37` -- The CLI accepts `-x` with a slash-separated multi-segment relative path value.
- `001.38` -- The CLI accepts repeated `-x` options.
- `001.39` -- The CLI rejects an `-x` path value with a leading `/`.
- `001.40` -- The CLI rejects an `-x` path value with a trailing `/`.
- `001.41` -- The CLI rejects an `-x` path value with a `\` separator.
- `001.42` -- The CLI rejects an `-x` path value with an empty path segment.
- `001.43` -- The CLI rejects an `-x` path value with a `.` path segment.
- `001.44` -- The CLI rejects an `-x` path value with a `..` path segment.
- `001.45` -- The CLI rejects an `-x` path value containing a NUL character.
- `001.46` -- On any non-help argument validation error, KitchenSync exits 1.
- `001.47` -- A non-help invocation rejects any value-taking option when its value is omitted.
- `001.48` -- The CLI accepts `-x <relative-path>` occurrences after the peer operands.

## Notes
This category owns the top-level CLI validation envelope. Exact no-argument help text and validation-error help text belong to `002_help-screen`; peer URL forms, fallback bracket syntax, peer role prefix placement, and per-URL query setting validation belong to `003_peer-addressing`; runtime first-sync and no-contributing-peer errors belong to `017_peer-roles-and-startup-state`; general output channel and logging rules belong to `014_logging-and-progress`; exclude effects during traversal belong to `007_traversal-and-excludes`; peer access without peer-side KitchenSync infrastructure belongs to `004_peer-connectivity`.
