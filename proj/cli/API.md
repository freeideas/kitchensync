# cli Module API

Rust module path: `kitchensync::cli`.

The `cli` module exposes command-line invocation parsing for the native
`kitchensync` executable. It is a leaf module: other modules may rely only on
the parse entry point, parse outcome, parse environment, and CLI validation
error contract described here. Scanner state, parser helpers, URL tokenization,
normalization helpers, defaults, and help text storage are private.

## Consumed Root Contracts

The `cli` module does not define duplicate domain types. Valid parse results
use the root-owned contracts:

- `RunRequest`
- `RunConfig`
- `PeerSpec`
- `PeerRole`
- `PeerUrl`
- `RelPath`
- `Verbosity`

The root owns these types because they are consumed by sibling modules after
CLI parsing succeeds.

## Public Types

```rust
pub struct CliParseEnv {
    pub current_dir: std::path::PathBuf,
    pub current_user: String,
}
```

`CliParseEnv` supplies the environment data required for parsing and
normalizing peer URL identity before root handoff.

- `current_dir` is used to resolve bare local paths and `file://` peer URLs to
  absolute file identities.
- `current_user` is inserted into SFTP peer URLs that omit a username.
- The parse caller owns the environment value and passes it by shared reference.
- The parser must not mutate the process environment or query global current
  directory or user state when this data is provided.

```rust
pub enum CliInvocation {
    Help {
        help: &'static str,
    },
    Invalid {
        error: CliArgumentError,
        help: &'static str,
    },
    Run(RunRequest),
}
```

`CliInvocation` is the complete result of parsing argv-style input.

- `Help` represents an empty argument list. Root prints `help` to stdout and
  exits with status `0`.
- `Invalid` represents an argument-validation failure. Root prints the
  user-facing error message followed by `help` to stdout and exits with status
  `1`. Stderr remains empty.
- `Run` contains a fully typed `RunRequest` with defaults applied, peer
  operands parsed and normalized, per-URL connection settings separated from
  identity, and excludes validated as `RelPath` values.

```rust
pub struct CliArgumentError {
    pub message: String,
}
```

`CliArgumentError` carries the exact user-facing validation message root needs
for stdout reporting.

- It represents command-line validation only, not runtime, transport,
  snapshot, sync, or filesystem failures.
- It owns its message because diagnostics may be assembled from argument text.
- It must not expose private parser tokens or implementation-specific error
  categories.

## Public Functions

```rust
pub fn parse_invocation<I, S>(
    args: I,
    env: &CliParseEnv,
) -> CliInvocation
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>;
```

Parses argv-style arguments after the executable name and returns one
`CliInvocation`.

Required behavior:

- Treat an empty argument iterator as `CliInvocation::Help`.
- Accept the public command shape
  `kitchensync [options] <peer> <peer> [<peer>...]`.
- Reject non-help invocations with fewer than two peer operands.
- Reject more than one explicit canon peer prefix.
- Parse global options, command-line excludes, peer role prefixes, fallback URL
  groups, peer URLs, and per-URL timeout query settings according to
  `proj/cli/SPEC.md`.
- Apply documented defaults only after validation succeeds.
- Preserve peer operand order, fallback URL order, and accepted exclude order.
- Normalize peer URL identity before constructing the returned `RunRequest`.
- Keep per-URL connection settings separate from normalized identity.
- Perform no peer connection, filesystem transport, snapshot, traversal,
  progress rendering, or sync decision work.

Ownership rules:

- The function consumes the provided argument iterator.
- Returned `RunRequest`, `RunConfig`, `PeerSpec`, `PeerUrl`, and `RelPath`
  values are owned by the caller through the root-owned contracts.
- Returned help text is borrowed static data.
- Returned validation errors own their message text.

```rust
pub fn help_text() -> &'static str;
```

Returns the exact CLI help payload used for empty-argv help and validation
failure reporting. Root may call this only for process output; sibling modules
must not parse or depend on help text content.

## Error Contract

All CLI failures are validation outcomes returned as
`CliInvocation::Invalid`. They must cover malformed command-line input such as:

- unknown flags;
- omitted option values;
- non-positive integer values where positive integers are required;
- unsupported verbosity values;
- invalid `-x` exclude paths;
- invalid peer operand syntax;
- unsupported URL forms;
- unsupported or omitted per-URL query parameter values;
- too few peer operands;
- more than one explicit canon peer.

No public CLI error may represent a failed peer connection, authentication
failure, host-key failure, snapshot failure, transport operation failure, sync
decision failure, or progress rendering failure.

## Visibility Rules

Public:

- `CliParseEnv`
- `CliInvocation`
- `CliArgumentError`
- `parse_invocation`
- `help_text`

Private:

- argument scanners;
- option parser state;
- exclude parser state;
- peer operand intermediate forms;
- URL parsing and normalization helpers;
- help payload storage details;
- defaulting helpers;
- diagnostic formatting helpers other than `CliArgumentError::message`.

Sibling modules consume the root-owned `RunRequest` and related root contracts
after a successful parse. They must not depend on `cli` parser internals.
