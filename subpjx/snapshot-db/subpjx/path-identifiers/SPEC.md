# Path Identifiers

## Purpose
Validate relative snapshot paths and derive stable path identifiers for rows and parent links.

## Public API
Data shapes:

- `RelativePath`: path relative to the peer root, using forward slashes, with no leading slash and no trailing slash
- `PathId`: xxHash64 seed 0 of a `RelativePath`, base62-encoded to 11 zero-padded characters
- `ParentId`: `PathId` of the parent `RelativePath`, or the `PathId` of `/` for root entries

Operations:

- `path_id(relative_path) -> PathId`
- `parent_id(relative_path) -> ParentId`

## Behavior
`path_id` hashes normalized relative paths with xxHash64 seed 0 and base62-encodes the 64-bit value to 11 zero-padded characters.

Directory and file paths hash identically.

The peer root itself is not represented by a row.

Root children use the hash of `/` as `parent_id`.

`parent_id` returns the `PathId` for the parent `RelativePath`. For a root child, it returns the `PathId` of `/`.

## Errors
Invalid relative paths return `invalid_path`.

Hashing or base62 encoding failures return `path_id_error`.

## Anchoring
`RelativePath`, `PathId`, `ParentId`, xxHash64 seed 0, base62 encoding, root sentinel `/`, and peer-root exclusion are anchored in `database.md` "Path Hashing".

xxHash64 is anchored by the xxHash64 algorithm.
