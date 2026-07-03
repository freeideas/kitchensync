# 001_cli-interface: CLI interface and help

## Behavior
This concern derives from `specs/README.md` sections "How to run" and
"Released artifacts", `specs/sync.md` sections "Command Line", "Global
Options", "URL Schemes", "Startup", and "Errors", and `specs/help.md` section
"Help Screen". It covers the observable released executable path, invocation
shape, help output, accepted flags, option defaults, peer argument syntax,
command-line validation, validation exit codes, and validation messages.

## $REQ_IDs
- `001.1` -- A release contains `released/kitchensync.exe` under `./released/`.
- `001.2` -- A release contains no files under `./released/` other than `released/kitchensync.exe`.
- `001.3` -- The file `released/kitchensync.exe` is directly invocable as the KitchenSync CLI.
- `001.4` -- The CLI accepts non-help invocations in the form `kitchensync [options] <peer> <peer> [<peer>...]`.
- `001.5` -- Running the CLI with no arguments prints the help screen from `specs/help.md` verbatim to stdout.
- `001.6` -- Running the CLI with no arguments exits 0.
- `001.7` -- Running the CLI with no arguments leaves stderr empty.
- `001.8` -- Non-help invocations require at least two peer arguments.
- `001.9` -- A peer argument with no URL scheme is treated as a `file://` URL.
- `001.10` -- Peer arguments accept local paths in `/path`, `c:\path`, and `./relative` forms.
- `001.11` -- Peer arguments accept SFTP URLs in `sftp://user@host/path` form.
- `001.12` -- Peer arguments accept SFTP URLs in `sftp://user@host:port/path` form.
- `001.13` -- Peer arguments accept SFTP URLs in `sftp://host/path` form.
- `001.14` -- Peer arguments accept SFTP URLs in `sftp://user:password@host/path` form.
- `001.15` -- SFTP URL passwords accept percent-encoded `@` and `:` characters.
- `001.16` -- SFTP URL paths identify absolute paths from the remote filesystem root.
- `001.17` -- The CLI accepts peer arguments prefixed with `+`.
- `001.18` -- The CLI accepts peer arguments prefixed with `-`.
- `001.19` -- The CLI accepts peer arguments with no prefix.
- `001.20` -- A non-help invocation accepts at most one canon peer.
- `001.21` -- A non-help invocation accepts multiple subordinate peers.
- `001.22` -- The CLI accepts bracketed fallback peer arguments in `[url1,url2,...]` form.
- `001.23` -- The CLI accepts canon fallback peer arguments in `+[url1,url2,...]` form.
- `001.24` -- The CLI accepts subordinate fallback peer arguments in `-[url1,url2,...]` form.
- `001.25` -- URL query strings accept the per-URL setting `timeout-conn`.
- `001.26` -- URL query strings accept the per-URL setting `timeout-idle`.
- `001.27` -- Non-help invocations reject URL query parameter names other than `timeout-conn` and `timeout-idle`.
- `001.28` -- The CLI accepts `--dry-run` as an option.
- `001.29` -- The default value of `--dry-run` is off.
- `001.30` -- The CLI accepts `--max-copies N` when `N` is a positive integer.
- `001.31` -- The default value of `--max-copies` is 10.
- `001.32` -- The CLI accepts `--retries-copy N` when `N` is a positive integer.
- `001.33` -- The default value of `--retries-copy` is 3.
- `001.34` -- The CLI accepts `--retries-list N` when `N` is a positive integer.
- `001.35` -- The default value of `--retries-list` is 3.
- `001.36` -- The CLI accepts `--timeout-conn N` when `N` is a positive integer.
- `001.37` -- The default value of `--timeout-conn` is 30.
- `001.38` -- The CLI accepts `--timeout-idle N` when `N` is a positive integer.
- `001.39` -- The default value of `--timeout-idle` is 30.
- `001.40` -- The CLI accepts `--verbosity error`.
- `001.41` -- The CLI accepts `--verbosity info`.
- `001.42` -- The CLI accepts `--verbosity debug`.
- `001.43` -- The CLI accepts `--verbosity trace`.
- `001.44` -- The default value of `--verbosity` is `info`.
- `001.45` -- The CLI accepts `-x RELPATH` as a repeatable option.
- `001.46` -- The CLI accepts `-x` values that are slash-separated relative paths.
- `001.47` -- Non-help invocations reject `-x` values with a leading `/`.
- `001.48` -- Non-help invocations reject `-x` values with a trailing `/`.
- `001.49` -- Non-help invocations reject `-x` values containing `\` separators.
- `001.50` -- Non-help invocations reject `-x` values containing empty path segments.
- `001.51` -- Non-help invocations reject `-x` values containing `.` path segments.
- `001.52` -- Non-help invocations reject `-x` values containing `..` path segments.
- `001.53` -- The CLI accepts `--keep-tmp-days N` when `N` is a positive integer.
- `001.54` -- The default value of `--keep-tmp-days` is 2.
- `001.55` -- The CLI accepts `--keep-bak-days N` when `N` is a positive integer.
- `001.56` -- The default value of `--keep-bak-days` is 90.
- `001.57` -- The CLI accepts `--keep-del-days N` when `N` is a positive integer.
- `001.58` -- The default value of `--keep-del-days` is 180.
- `001.59` -- Non-help invocations reject unrecognized flags.
- `001.60` -- Non-help invocations reject invalid option values.
- `001.61` -- On any command-line validation error, the CLI prints an error message followed by the help screen to stdout.
- `001.62` -- On any command-line validation error, the CLI exits 1.
- `001.63` -- On any command-line validation error, the CLI leaves stderr empty.

## Notes
This category owns parsing and validation of command-line text. Later startup
behavior after arguments have been accepted belongs to
`002_peer-startup-and-identity`.

The `-x` NUL-character rule from `specs/sync.md` is not represented as a CLI
requirement because supported operating systems do not allow NUL characters in
process arguments.
