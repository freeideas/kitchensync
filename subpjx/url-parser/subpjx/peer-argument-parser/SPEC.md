# Peer Argument Parser

## Purpose
Parse one peer argument into its peer role and ordered URL texts, including prefix modifiers and fallback URL groups.

## Public API
Data shapes:

- `PeerRole`: `canon`, `subordinate`, or `bidirectional`
- `Peer`: `role`, ordered `urls`
- `UrlText`: one URL or bare path text

Operations:

- `parse_peer(text) -> Peer`

## Behavior
`parse_peer` accepts a single peer argument.

A leading `+` sets `role = canon`; a leading `-` sets `role = subordinate`; no prefix sets `role = bidirectional`.

Square brackets group fallback URLs into one `Peer`. The returned `urls` preserve fallback URL order. A prefix applies to the whole fallback group.

Without a fallback group, the returned `Peer` contains one `UrlText`.

Returned `UrlText` values exclude any prefix modifier and fallback brackets. URL text is otherwise preserved as input text for each URL.

Scheme dispatch, per-URL settings, URL normalization, and URL identity are not interpreted by this operation.

## Errors
Invalid input returns one of:

- `invalid_peer`
- `invalid_fallback_group`
- `invalid_prefix`

`invalid_peer` is returned for an empty peer argument.

`invalid_fallback_group` is returned for malformed fallback brackets or a fallback group with no URL text.

`invalid_prefix` is returned when a prefix modifier is repeated or is not attached to the peer argument.

## Anchoring
`parse_peer`, `Peer`, `PeerRole`, `role`, prefix modifiers, and peer argument are anchored in `sync.md` "Peers".

`urls`, `UrlText`, fallback groups, fallback URL order, and bare path text are anchored in `sync.md` "Fallback URLs" and "URL Schemes".

URL text is anchored in RFC 3986.
