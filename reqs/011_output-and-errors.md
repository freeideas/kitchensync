# 011_output-and-errors: Output and errors

## Behavior
This concern derives from `specs/README.md` section "How to run",
`specs/help.md` section "Help Screen", `specs/sync.md` sections "Startup",
"Run", "Logging", and "Errors", `specs/concurrency.md` sections "Progress
Output" and "Trace Logging", and `specs/SCENARIOS.md` scenarios S-01 through
S-11 and property "P-01: Output Channels". It covers stdout-only output,
empty stderr, completion output, progress line format and verbosity gating,
trace copy-slot line format, error diagnostics, transfer failure diagnostics,
and process exit codes for successful and failed invocations.

## $REQ_IDs
- `011.1` -- All output produced by KitchenSync is written to stdout.
- `011.2` -- stderr is empty across argument parsing, sync execution, and shutdown.
- `011.3` -- Running `kitchensync` with no arguments writes the help screen to stdout.
- `011.4` -- Running `kitchensync` with no arguments exits 0.
- `011.5` -- An argument validation error on a non-help invocation writes the validation error message followed by the help screen to stdout.
- `011.6` -- An argument validation error on a non-help invocation exits 1.
- `011.7` -- A successful sync writes exactly one completion line, `sync complete`, to stdout.
- `011.8` -- The `sync complete` completion line is emitted at every verbosity level.
- `011.9` -- A successful sync exits 0.
- `011.10` -- `--verbosity error` suppresses `C` and `X` progress lines.
- `011.11` -- `--verbosity info`, `--verbosity debug`, and `--verbosity trace` emit progress lines for copy and delete actions.
- `011.12` -- A copy progress line is formatted as `C <relpath>`, where `<relpath>` is the slash-separated relative path from the sync root.
- `011.13` -- A delete progress line is formatted as `X <relpath>`, where `<relpath>` is the slash-separated relative path from the sync root.
- `011.14` -- Copy progress emits one `C <relpath>` line per copied path, regardless of how many peers receive that path.
- `011.15` -- Delete progress emits one `X <relpath>` line per deleted path, regardless of how many peers displace that path.
- `011.16` -- Progress lines are emitted in the order the actions happen.
- `011.17` -- Progress output omits directory creation, directory listing, snapshot work, and BAK/TMP cleanup.
- `011.18` -- Progress output uses plain lines rather than a live status screen, progress bar, percentage, scanned-directory indicator, or terminal control sequence.
- `011.19` -- Progress output is identical whether stdout is a terminal or a pipe.
- `011.20` -- Each higher verbosity level emits all messages defined for lower verbosity levels.
- `011.21` -- With the current specification, `--verbosity debug` produces the same observable output as `--verbosity info`.
- `011.22` -- `--verbosity trace` emits a copy-slot line when a global copy slot is acquired.
- `011.23` -- `--verbosity trace` emits a copy-slot line when a global copy slot is released.
- `011.24` -- A copy-slot trace line is formatted as `copy-slots active=<n>/<max>`.
- `011.25` -- Copy-slot trace lines report global active file-copy slots rather than network connections.
- `011.26` -- Error-level diagnostics are emitted at `error`, `info`, `debug`, and `trace` verbosity.
- `011.27` -- If no reachable peer has snapshot history and no canon peer is designated, KitchenSync writes exactly `First sync? Mark the authoritative peer with a leading +` as one stdout line.
- `011.28` -- If no reachable peer has snapshot history and no canon peer is designated, KitchenSync exits 1.
- `011.29` -- If no contributing peer is reachable after auto-subordination, KitchenSync writes exactly `No contributing peer reachable - cannot make sync decisions` as one stdout line.
- `011.30` -- If no contributing peer is reachable after auto-subordination, KitchenSync exits 1.
- `011.31` -- If fewer than two peers are reachable during startup, KitchenSync exits 1.
- `011.32` -- If the canon peer is unreachable during startup, KitchenSync exits 1.
- `011.33` -- An unreachable peer emits an error-level diagnostic.
- `011.34` -- A snapshot recovery or snapshot download failure emits an error-level diagnostic before that peer is excluded from the reachable set.
- `011.35` -- A directory listing failure that remains after all allowed listing tries emits an error-level diagnostic.
- `011.36` -- A transfer failure that exhausts its allowed copy tries emits an error-level diagnostic.
- `011.37` -- A transfer failure after SWAP `old` exists emits an error-level diagnostic.
- `011.38` -- An archive-old failure emits an error-level diagnostic.
- `011.39` -- A displacement failure emits an error-level diagnostic.
- `011.40` -- A `set_mod_time` failure emits an error-level diagnostic.
- `011.41` -- A snapshot upload failure before SWAP `old` exists emits an error-level diagnostic.
- `011.42` -- A snapshot upload failure after SWAP `old` exists emits an error-level diagnostic.
- `011.43` -- A failed file-transfer diagnostic identifies the transfer's relative path.
- `011.44` -- A failed file-transfer diagnostic identifies the destination peer URL.
- `011.45` -- A failed file-transfer diagnostic identifies the failed phase as one of `read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`.
- `011.46` -- A failed file-transfer diagnostic identifies the transport error category when a transport error category is available.

## Notes
This category owns output channels, diagnostic formats, and exit status. The
exact help text belongs to `001_cli-interface`; the dry-run preface belongs to
`012_dry-run`; the state changes that cause messages belong to the category for
the underlying operation.
