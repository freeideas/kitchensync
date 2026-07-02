# 001_cli-release-and-invocation: Released CLI executable and invocation shape

## Behavior
This concern derives from `specs/README.md` sections "How to run" and
"Released artifacts", and from `specs/sync.md` opening section. It covers the
observable `released/kitchensync.exe` artifact, the rule that `./released/`
contains exactly that shipped file, the `kitchensync [options] <peer> <peer>
[<peer>...]` process shape, and the fact that KitchenSync is a native
command-line program rather than a service.

## $REQ_IDs
- `001.1` -- A release build writes exactly one shipped file under `./released/`: `released/kitchensync.exe`.
- `001.2` -- `released/kitchensync.exe` is directly invocable from `./released/` as the KitchenSync command-line executable.
- `001.3` -- KitchenSync is delivered as a native command-line executable for Windows, Linux, and macOS rather than as a service.
- `001.4` -- KitchenSync invocations place options before peer operands.
- `001.5` -- KitchenSync invocations include at least two peer path or URL operands.
- `001.6` -- KitchenSync invocations accept additional peer path or URL operands after the first two peer operands.

## Notes
This file is about the shipped process and artifact boundary. Argument parsing,
help text, logging, and sync behavior belong to later categories.
