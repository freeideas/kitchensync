# 002_help-screen: Help screen output

## Behavior
This concern derives from `specs/help.md` section "Help Screen" and `specs/sync.md` sections "Command Line" and "Startup". It covers the exact help text printed for a no-argument invocation, the no-argument stdout/stderr and exit behavior, and reuse of that same help text after non-help validation errors.

## $REQ_IDs

- `002.1` -- Running `kitchensync` with no arguments writes to stdout the help text block from `specs/help.md` section "Help Screen" exactly as written.
- `002.2` -- Running `kitchensync` with no arguments exits 0.
- `002.3` -- Running `kitchensync` with no arguments leaves stderr empty.
- `002.4` -- A non-help invocation rejected during command-line validation writes the exact help text from `002.1` to stdout after its validation error message.
- `002.5` -- A non-help invocation rejected during command-line validation exits 1.

## Notes
This category owns the verbatim help surface. It does not own which non-help inputs are invalid except where that is represented in the help text; validation ownership belongs to `001_cli-interface` and peer-address-specific validation belongs to `003_peer-addressing`.
