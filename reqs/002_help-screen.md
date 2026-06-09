# 002_help-screen: Help screen output

## Behavior
This concern derives from `specs/help.md` and `specs/sync.md` section "Startup"
step 1.

It covers the no-argument invocation: KitchenSync prints the exact help text
verbatim to stdout and exits 0, with stderr empty. It also covers that the same
help text is appended after the error message when a non-help invocation fails
validation. The requirement is the literal, character-for-character help screen
content.

The decision of when an invocation counts as a validation error (versus a help
invocation) and the error messages themselves belong to `001_command-line`.

## $REQ_IDs

- `002.1` -- Running `kitchensync` with no arguments prints to stdout the help text exactly as written in `specs/help.md`, character for character.
- `002.2` -- Running `kitchensync` with no arguments exits 0.
- `002.3` -- Running `kitchensync` with no arguments leaves stderr empty.
- `002.4` -- When a non-help invocation fails argument validation, the help text exactly as written in `specs/help.md` is printed after the error message.
