# 02_file-copy: File copy with TMP staging and atomic swap

## Behavior

Each file copy stages new content into `.kitchensync/TMP/` on the destination, displaces any existing target into `.kitchensync/BAK/`, atomically renames the staged file into place, and sets its mod_time to the winning decision value. Content is streamed via a bounded channel between concurrent reader and writer tasks. Derived from `specs/sync.md` §"File Copy" and §"Peer Transports".

## $REQ_IDs
- `02.29` — A file copy first writes the source content to `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>` on the destination.
- `02.30` — If the destination already has a file at the target path, that existing file is renamed to `<file-parent>/.kitchensync/BAK/<timestamp>/<basename>` before the swap.
- `02.31` — After staging (and any displacement), the staged file is renamed from its TMP path to the final target path via a same-filesystem (atomic) rename.
- `02.32` — The destination file's modification time is set to the winning peer's mod_time from the decision, not re-read from the source.
- `02.33` — Each transfer uses two concurrent tasks — a reader pulling chunks from the source and a writer pushing chunks to the destination — connected by a bounded channel, not a single sequential read-then-write loop.
- `02.34` — On transfer failure, the TMP staging file/directory for that transfer is deleted before connections are returned to their pools.
- `02.47` — After a successful copy, the per-transfer TMP scaffolding (`<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/`) is removed.

## Notes
The `<timestamp>` in TMP and BAK paths uses the format defined in `specs/database.md` (`YYYY-MM-DD_HH-mm-ss_ffffffZ`). The reader/writer-channel structure is verified by code examination per `specs/concurrency.md` §"Pipelined transfer test".
