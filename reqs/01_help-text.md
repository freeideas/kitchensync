# 01_help-text: Help screen behavior

## Behavior

When invoked with no arguments, `-h`, `--help`, or `/?`, the program prints the help text to stdout and exits 0. Help output goes to stdout; stderr is empty. Derived from `help.md` and `sync.md` §Startup.

## $REQ_IDs

- `01.1` — Running with no arguments prints the help text and exits 0.
- `01.2` — Running with `-h` prints the help text and exits 0.
- `01.3` — Running with `--help` prints the help text and exits 0.
- `01.4` — Running with `/?` prints the help text and exits 0.
- `01.5` — Help output goes to stdout.
- `01.17` — When help is printed, stderr is empty.
- `01.6` — Help text describes peer URL forms (local paths, `sftp://user@host/path`, port and password variants).
- `01.7` — Help text describes the `+` (canon) and `-` (subordinate) prefix modifiers.
- `01.8` — Help text describes fallback URL bracket syntax.
- `01.9` — Help text lists the global option flags (`--mc`, `--ct`, `--ka`, `-vl`, `--xd`, `--bd`, `--td`) with their defaults.
- `01.25` — Help text describes per-URL query string settings.

## Notes

The help text content is mandated by `help.md`; keep the embedded text and what the program prints in sync.
