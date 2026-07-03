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

## Notes
This category owns output channels, diagnostic formats, and exit status. The
exact help text belongs to `001_cli-interface`; the dry-run preface belongs to
`012_dry-run`; the state changes that cause messages belong to the category for
the underlying operation.

## $REQ_IDs
