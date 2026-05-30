# Cli Module Architecture

The `cli` module owns interpretation of the native `kitchensync` process
arguments. It converts argv-style input into one of three outcomes: empty-argv
help, an argument-validation failure with the exact help text, or a valid typed
`RunRequest` for root orchestration.

This module is a leaf. Its scope is limited to invocation interpretation, and
the expected implementation units are private helpers rather than child modules.
Splitting child modules here would expose parser details without creating a
useful tree-scoped contract.

## Responsibilities

- Preserve and parse the public command shape
  `kitchensync [options] <peer> <peer> [<peer>...]`.
- Select the exact no-argument help text and the exact help text paired with
  validation failures.
- Validate global option names, option values, command-line excludes, peer
  operand count, peer role prefixes, fallback URL syntax, peer URL syntax, and
  per-URL query settings.
- Apply CLI defaults for omitted options only after the invocation is otherwise
  valid.
- Convert accepted arguments into a root-owned `RunRequest` containing
  `RunConfig`, ordered `PeerSpec` values, parsed `PeerUrl` values, explicit
  peer roles, per-URL connection settings, and accepted excludes.
- Normalize the peer URL identity fields required before root handoff while
  keeping per-URL connection settings separate from identity comparison.
- Report argument-validation failures in a form that lets the root print the
  specific error followed by help to stdout and exit with the required status.

The module does not connect to peers, choose fallback winners, authenticate SSH,
verify host keys, inspect snapshots, apply startup-derived peer roles, schedule
copies, apply excludes during traversal, render progress, or make sync
decisions.

## Internal Design

The implementation should stay as a compact set of private units:

- Argument scanner: walks raw argument strings, separates global options,
  option values, `-x` exclude values, and peer operands while preserving source
  text for diagnostics.
- Global option parser: recognizes `--dry-run`, positive-integer options,
  `--verbosity`, omitted values, and unknown flags, then builds the option
  portion of `RunConfig` with documented defaults.
- Exclude parser: validates each `-x` value through the shared `RelPath`
  contract and preserves accepted excludes in command-line order.
- Peer operand parser: parses a whole peer operand into its explicit role marker
  and one or more URL texts, including bracketed fallback groups whose prefix
  applies to the peer rather than individual URLs.
- URL parser and normalizer: parses bare paths, `file://` URLs, supported SFTP
  URL forms, inline SFTP credentials, and per-URL query parameters; it produces
  normalized identity fields and separate connection settings.
- Validator: applies cross-argument checks such as minimum peer count and at
  most one explicit canon peer, then constructs the final invocation result.
- Help provider: supplies the stable help payload selected by help and
  validation outcomes.

These units communicate through private structs and functions. The only
module-external API is the parse result consumed by the root.

## Data Flow

1. Root passes argv-style strings to the CLI parse entry point.
2. Empty argv returns the help outcome without peer or option validation.
3. Non-empty argv is scanned into global option tokens, exclude tokens, and
   peer operand tokens.
4. Options and excludes are parsed and validated into typed intermediate
   values.
5. Peer operands are parsed into role markers and URL candidates. Fallback URL
   order is preserved exactly as supplied.
6. Each URL candidate is parsed, validated, normalized for identity, and paired
   with any accepted per-URL timeout settings.
7. Cross-argument validation rejects invalid arity, multiple explicit canon
   peers, and any malformed syntax or values.
8. A valid invocation returns `RunRequest`; an invalid invocation returns a CLI
   validation error plus help text.

Invalid invocations stop at this boundary. No startup, peer connection,
snapshot, traversal, copy, or progress work may happen after a CLI validation
failure.

## URL And Path Handling

Peer operands with no `+` or `-` prefix are normal peers. A leading `+` marks
the whole peer as canon, and a leading `-` marks the whole peer as explicitly
subordinate. Bracketed fallback groups such as `[url1,url2]`, `+[url1,url2]`,
and `-[url1,url2]` produce one peer with URLs kept in command-line order.
Individual URLs inside fallback groups must not carry role prefixes.

Bare paths are accepted as local `file://` peer URLs, including Unix absolute
paths, Windows drive paths, and relative paths. `file://` URLs are accepted as
local peer URLs. Supported SFTP forms are:
`sftp://user@host/path`, `sftp://user@host:port/path`,
`sftp://host/path`, and `sftp://user:password@host/path`.

Before handoff, identity normalization lowercases scheme and hostname, removes
default SFTP port `22`, collapses consecutive path slashes, removes trailing
path slashes, converts bare paths to `file://`, resolves `file://` URLs to
absolute paths from the current working directory, percent-decodes unreserved
characters, strips query parameters from normalized identity, and inserts the
current OS user into SFTP URLs that omit a username. Inline SFTP password text
is preserved after the URL decoding needed for `%40` and `%3A`.

Per-URL query parameters are parsed on every URL, including fallback URLs. Only
`timeout-conn` and `timeout-idle` are accepted, both require positive integers,
and each setting applies only to the URL on which it appears. Connection
settings remain separate from normalized peer identity.

## Dependencies

The `cli` module consumes root-owned shared contracts:

- `RunRequest` and `RunConfig` for valid handoff to root orchestration.
- `PeerSpec`, `PeerRole`, and `PeerUrl` for ordered peer operands, explicit
  roles, fallback URLs, normalized URL identity, and per-URL settings.
- `RelPath` for command-line excludes.
- `Verbosity` for `--verbosity` values.

The module may use language-standard string and path parsing support. It must
not depend on peer connectivity, transport, snapshot, sync, operations, or
runtime implementation details. Sibling modules consume the root-owned typed
request, not CLI-private scanner or parser state.

## Visibility

Public surface:

- A parse entry point that accepts raw process arguments and the environment
  context needed for current-directory and current-user normalization.
- A parsed invocation result with help, validation-error-with-help, and valid
  request variants.
- CLI validation error data containing the user-facing message needed by root
  process handling.

Private surface:

- Token classification, parser state, source-text helpers, option defaults,
  help payload storage, URL parsing helpers, normalization helpers, and peer
  operand intermediate forms.

Shared contracts belong at the root because the parsed request is consumed by
multiple sibling modules. CLI-specific parsing and validation helpers remain
private to this module.

## Error Handling

CLI errors are argument-validation results, not runtime failures. They carry a
user-facing message and the help text required by the stdout-only process
contract. They do not use transport, snapshot, sync, or runtime error
categories because no peer I/O has happened.

Validation must reject unknown flags, missing option values, non-positive
integer values where positive integers are required, unsupported verbosity
values, invalid exclude paths, invalid peer operands, unsupported URL forms,
unsupported per-URL query parameters, and omitted per-URL query values.
Defaults are applied only for valid invocations so downstream modules always
receive a complete typed configuration.

## Future Change Guidance

Keep future changes limited to command-line syntax, validation, help selection,
URL/exclude parsing, and parsed run configuration. If a new option affects
behavior in another module, CLI should parse and validate the option, then place
the typed value in the narrowest root-owned contract that needs it. The owning
sibling module still decides the domain behavior.
