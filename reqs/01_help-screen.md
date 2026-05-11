# 01_help-screen: Help text prints verbatim and exits 0

## Behavior

Four trigger conditions — `-h`, `--help`, `/?`, or no arguments at all — cause KitchenSync to print the help text verbatim to stdout and exit 0. The text is the block embedded in the JAR at build time. Derived from `specs/help.md` and `specs/sync.md` §"Command Line".

## $REQ_IDs
- `01.1` — Running with no arguments prints the help text to stdout and exits 0.
- `01.2` — `-h` prints the help text to stdout and exits 0.
- `01.3` — `--help` prints the help text to stdout and exits 0.
- `01.4` — `/?` prints the help text to stdout and exits 0.
- `01.5` — On help triggers, stderr is empty (all output is on stdout).
- `01.6` — The printed help text matches the verbatim block in `specs/help.md`.

## Notes
The verbatim block already covers URL forms, prefix modifiers, fallback bracket syntax, per-URL settings, and global option defaults, so `01.6` is the contractual content check.
