# 001_cli-release-and-invocation: Released CLI executable and invocation shape

## Behavior
This concern derives from `specs/README.md` sections "How to run" and
"Released artifacts", and from `specs/sync.md` opening section. It covers the
observable `released/kitchensync.exe` artifact, the rule that `./released/`
contains exactly that shipped file, the `kitchensync [options] <peer> <peer>
[<peer>...]` process shape, and the fact that KitchenSync is a native
command-line program rather than a service.

## $REQ_IDs
- `001.1` -- On Windows, Linux, and macOS, a release build creates `released/kitchensync.exe`.
- `001.2` -- On Windows, Linux, and macOS, a release build leaves no files under `./released/` other than `released/kitchensync.exe`.
- `001.3` -- `released/kitchensync.exe` is directly invocable from `./released/` as the KitchenSync command-line executable.
- `001.4` -- KitchenSync runs as a native command-line executable rather than as a service.
- `001.5` -- KitchenSync command invocations place options before peer operands.
- `001.6` -- KitchenSync command invocations accept peer operands that are paths or URLs.
- `001.7` -- KitchenSync command invocations accept at least two peer operands.
- `001.8` -- KitchenSync command invocations accept additional peer operands after the first two peer operands.

## Notes
This file is about the shipped process and artifact boundary. Argument parsing,
help text, logging, and sync behavior belong to later categories.
