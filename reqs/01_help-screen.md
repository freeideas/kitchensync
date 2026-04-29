# 01_help-screen: Help text printed to stdout, exit 0

## Behavior

When the user invokes the program with no arguments or with `-h`, `--help`, or `/?`, the program prints the help text verbatim to stdout, leaves stderr empty, and exits 0. Derived from `./specs/help.md` and the `Command Line` section of `./specs/sync.md`.

## $REQ_IDs
- `01.1` — Running with no arguments prints help to stdout and exits 0.
- `01.2` — Running with `-h` prints help to stdout and exits 0.
- `01.3` — Running with `--help` prints help to stdout and exits 0.
- `01.4` — Running with `/?` prints help to stdout and exits 0.
- `01.5` — When help is printed, stderr is empty.
- `01.6` — Help output matches the embedded help text from `./specs/help.md` verbatim.

## Notes

The help text is embedded in the JAR at build time (`./specs/help.md`).
