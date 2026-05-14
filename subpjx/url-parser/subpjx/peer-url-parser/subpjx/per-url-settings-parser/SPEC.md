# Per-URL Settings Parser

## Purpose
Parse per-URL query parameters into `PerUrlSettings`.

## Public API
Data shapes:

- `PerUrlSettings`: optional `mc`, `ct`, `ka`

Operations:

- `parse_per_url_settings(query) -> PerUrlSettings`

## Behavior
`parse_per_url_settings` reads query-string parameters `mc`, `ct`, and `ka`.

When present, `mc`, `ct`, and `ka` are parsed as positive integers and returned in `PerUrlSettings`.

Absent settings are omitted from `PerUrlSettings`.

## Errors
Invalid input returns:

- `invalid_setting`

Settings `mc`, `ct`, and `ka` must be positive integers when present.

## Anchoring
`PerUrlSettings` and settings `mc`, `ct`, and `ka` are anchored in `sync.md` "Per-URL Settings".

Query strings and query parameters are anchored in RFC 3986.
