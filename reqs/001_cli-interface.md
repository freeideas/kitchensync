# 001_cli-interface: CLI interface and help

## Behavior
This concern derives from `specs/README.md` sections "How to run" and
"Released artifacts", `specs/sync.md` sections "Command Line", "Global
Options", "URL Schemes", "Startup", and "Errors", and `specs/help.md` section
"Help Screen". It covers the observable released executable path, invocation
shape, help output, accepted flags, option defaults, peer argument syntax,
command-line validation, validation exit codes, and validation messages.

## Notes
This category owns parsing and validation of command-line text. Later startup
behavior after arguments have been accepted belongs to
`002_peer-startup-and-identity`.

## $REQ_IDs
