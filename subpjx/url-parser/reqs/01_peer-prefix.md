# 01_peer-prefix: Detect the +/-/normal prefix on a peer argument

## Behavior

`parse_peer_arg` inspects the leading character of the raw argument and tags the returned `Peer` with one of three prefix kinds. The `+` and `-` markers belong to the argument as a whole and are not part of the URL content that follows them. Derived from `SPEC.md` §"Peer-argument parsing" (the `prefix` field of the returned `Peer`).

## $REQ_IDs
- `01.1` — A peer argument with no leading `+` or `-` produces a `Peer` whose prefix is `normal`.
- `01.2` — A peer argument starting with `+` produces a `Peer` whose prefix is `canon`.
- `01.3` — A peer argument starting with `-` produces a `Peer` whose prefix is `subordinate`.
