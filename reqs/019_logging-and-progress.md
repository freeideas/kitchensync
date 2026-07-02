# 019_logging-and-progress: Stdout diagnostics, progress, and verbosity

## Behavior
This concern derives from `specs/README.md` section "How to run",
`specs/sync.md` sections "Logging" and "Errors", and `specs/concurrency.md`
sections "Progress Output" and "Trace Logging". It covers stdout-only output,
empty stderr, ordered per-action `C` and `X` progress lines, verbosity level
filtering, trace copy-slot events, error-level diagnostics, failed transfer
diagnostic fields, completion logging, and the requirement that output is the
same whether or not stdout is a terminal.

## $REQ_IDs
- `019.1` -- All output produced by KitchenSync is written to stdout.
- `019.2` -- KitchenSync leaves stderr empty during argument parsing, sync execution, and shutdown.
- `019.3` -- Each error condition enumerated in `specs/sync.md` section "Errors" emits an error-level diagnostic.
- `019.4` -- A run with no snapshot history and no canon peer prints a stdout diagnostic that suggests marking a canon peer with `+`.
- `019.5` -- A run with no contributing peer reachable prints `No contributing peer reachable - cannot make sync decisions` to stdout.
- `019.6` -- Failed file-transfer diagnostics identify the slash-separated relative path.
- `019.7` -- Failed file-transfer diagnostics identify the destination peer URL.
- `019.8` -- Failed file-transfer diagnostics identify the failed transfer phase.
- `019.9` -- Failed file-transfer diagnostics identify the transport error category when that category is available.
- `019.10` -- Failed file-transfer diagnostics use one of these failed phase labels: `read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`.
- `019.11` -- During sync execution, each progress action emits one plain line to stdout in the order the action happens.
- `019.12` -- Each progress line contains the action letter, one space, and the slash-separated relative path from the sync root.
- `019.13` -- A copied file path emits one `C <relpath>` progress line regardless of how many destination peers receive the file.
- `019.14` -- A path displaced to BAK emits one `X <relpath>` progress line regardless of how many peers displace it.
- `019.15` -- Displaced files and displaced directories both use the `X <relpath>` progress line format.
- `019.16` -- Directory creation emits no `C` or `X` progress line.
- `019.17` -- Directory listing emits no `C` or `X` progress line.
- `019.18` -- Snapshot work emits no `C` or `X` progress line.
- `019.19` -- BAK, TMP, and SWAP cleanup emit no `C` or `X` progress line.
- `019.20` -- Progress output contains no live status screen, progress bar, percentage, scanned-directory indicator, or terminal control sequence.
- `019.21` -- KitchenSync produces the same output whether stdout is a terminal or a redirected stream.
- `019.22` -- Verbosity levels are cumulative in this order: `error`, `info`, `debug`, `trace`.
- `019.23` -- `C` and `X` progress lines are info-level output.
- `019.24` -- `--verbosity error` suppresses info-level `C` and `X` progress lines.
- `019.25` -- `--verbosity debug` produces the same observable output as `--verbosity info`.
- `019.26` -- `--verbosity trace` includes copy-slot acquire and release events.
- `019.27` -- Each trace copy-slot event is emitted as `copy-slots active=<n>/<max>`.
- `019.28` -- Trace copy-slot events report global active file-copy slots rather than network connection counts.
- `019.29` -- A successful sync execution emits exactly one completion line to stdout.
- `019.30` -- The successful-sync completion line is `sync complete`.
- `019.31` -- The `sync complete` completion line is emitted at every verbosity level.

## Notes
This file covers how observable messages are emitted. It does not decide which
sync action should happen.
