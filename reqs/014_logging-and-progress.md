# 014_logging-and-progress: Logging, diagnostics, and progress output

## Behavior
This concern derives from `specs/sync.md` sections "Logging" and "Errors", `specs/concurrency.md` sections "Live Terminal Status" and "Trace Logging", and `specs/README.md` section "How to run". It covers stdout-only diagnostics, empty stderr, verbosity level behavior, live interactive progress screen content and refresh rate, non-interactive progress output, current scanned directory display, failed transfer diagnostic content, trace copy-slot logs, visibility of errors and completion messages, and process exit observability.

## $REQ_IDs
- `014.1` -- KitchenSync writes all diagnostics, progress output, and completion output to stdout.
- `014.2` -- KitchenSync leaves stderr empty across argument parsing, sync execution, and shutdown.
- `014.3` -- On any non-help argument validation error, KitchenSync prints the error message followed by the help text to stdout.
- `014.4` -- On any non-help argument validation error, KitchenSync exits 1.
- `014.5` -- When no peer has snapshot history and no canon peer is designated, KitchenSync prints `First sync? Mark the authoritative peer with a leading +` to stdout.
- `014.6` -- When no peer has snapshot history and no canon peer is designated, KitchenSync exits 1.
- `014.7` -- When no contributing peer is reachable after auto-subordination, KitchenSync prints `No contributing peer reachable - cannot make sync decisions` to stdout.
- `014.8` -- When no contributing peer is reachable after auto-subordination, KitchenSync exits 1.
- `014.9` -- When the canon peer is unreachable, KitchenSync exits 1.
- `014.10` -- When fewer than two peers are reachable, KitchenSync exits 1.
- `014.11` -- On successful sync completion, KitchenSync logs a completion message to stdout.
- `014.12` -- On successful sync completion, KitchenSync exits 0.
- `014.13` -- An unreachable peer produces an error-level diagnostic on stdout.
- `014.14` -- A directory listing failure that exhausts `--retries-list` produces an error-level diagnostic on stdout.
- `014.15` -- A file transfer failure before SWAP `old` exists produces a final error-level diagnostic on stdout when the transfer has exhausted `--retries-copy`.
- `014.16` -- A file transfer failure after SWAP `old` exists produces an error-level diagnostic on stdout.
- `014.17` -- An archive-old failure after replacement produces an error-level diagnostic on stdout.
- `014.18` -- A displacement failure produces an error-level diagnostic on stdout.
- `014.19` -- A TMP or SWAP staging failure is reported as a transfer failure on stdout.
- `014.20` -- A `set_mod_time` failure after a completed copy produces an error-level diagnostic on stdout.
- `014.21` -- A snapshot upload failure before SWAP `old` exists produces an error-level diagnostic on stdout.
- `014.22` -- A snapshot upload failure after SWAP `old` exists produces an error-level diagnostic on stdout.
- `014.23` -- A failed file-transfer diagnostic identifies the transfer's relative path.
- `014.24` -- A failed file-transfer diagnostic identifies the destination peer URL.
- `014.25` -- A failed file-transfer diagnostic identifies the failed transfer phase.
- `014.26` -- A failed file-transfer diagnostic identifies the transport error category when that category is available.
- `014.27` -- A failed file-transfer diagnostic reports its failed phase as one of `read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`.
- `014.28` -- When `--verbosity` is not specified, KitchenSync uses `info` verbosity.
- `014.29` -- `--verbosity error` emits error-level diagnostics and omits info-level progress output.
- `014.30` -- `--verbosity info` emits error-level diagnostics and info-level progress output.
- `014.31` -- `--verbosity debug` is observationally identical to `--verbosity info`.
- `014.32` -- `--verbosity trace` emits the defined error-level diagnostics, info-level progress output, and trace-level copy-slot events.
- `014.33` -- At `--verbosity trace`, KitchenSync logs copy-slot acquire events.
- `014.34` -- At `--verbosity trace`, KitchenSync logs copy-slot release events.
- `014.35` -- Each trace copy-slot event uses the format `copy-slots active=<n>/<max>`.
- `014.36` -- Trace copy-slot events report global active file-copy slots rather than network connections.
- `014.37` -- During an interactive sync run at info-or-more-verbose output, KitchenSync displays progress through a live terminal status screen.
- `014.38` -- The live terminal status screen updates at no more than once per second.
- `014.39` -- The live terminal status screen coalesces faster internal events into the next refresh.
- `014.40` -- The live terminal status screen shows one row for each active file copy, up to the configured `--max-copies` limit.
- `014.41` -- Each active-copy row on the live terminal status screen starts with the copied file's basename.
- `014.42` -- Each active-copy row on the live terminal status screen omits the copied file's full path before the progress bar.
- `014.43` -- Each active-copy row on the live terminal status screen shows a horizontal progress bar after the basename.
- `014.44` -- Each active-copy row's progress bar grows toward completion as bytes are copied.
- `014.45` -- When a file is fully copied, its live progress bar reaches the end before the row disappears or is replaced.
- `014.46` -- The bottom line of the live terminal status screen is always the directory currently being scanned.
- `014.47` -- While scanning the root directory, the live terminal status screen displays `Scanning: .`.
- `014.48` -- While scanning a non-root directory, the live terminal status screen displays the full slash-separated relative directory path from the sync root.
- `014.49` -- If the live terminal status screen shows completed counts, failed counts, or an overall percentage, those summaries do not displace active-copy rows or the bottom scanning line.
- `014.50` -- When stdout is not an interactive terminal, KitchenSync emits no terminal control sequences.
- `014.51` -- When stdout is not an interactive terminal, KitchenSync emits plain line-oriented progress at no more than once per second.
- `014.52` -- When stdout is not an interactive terminal, KitchenSync includes the currently scanned directory in line-oriented progress output.
- `014.53` -- Errors remain visible after the live terminal status screen finishes.
- `014.54` -- Final completion messages remain visible after the live terminal status screen finishes.

## Notes
This category owns how KitchenSync reports behavior. It does not own the underlying sync decisions or transfer scheduling that produce those reports.
