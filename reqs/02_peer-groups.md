# Peer Groups

Formation, recognition, and management of peer groups.

## $REQ_GRP_004: Minimum Group Size
**Source:** ./specs/sync.md (Section: "Startup")

A group must have at least two peers.

## $REQ_GRP_005: CLI-Driven Group Resolution — Known URL
**Source:** ./specs/database.md (Section: "CLI-driven group resolution")

When a CLI URL matches an existing peer in `peer_url`, the peer's group is loaded from the config file as the active group for this run.

## $REQ_GRP_006: CLI-Driven Group Resolution — New URL Added
**Source:** ./specs/database.md (Section: "CLI-driven group resolution")

New URLs (not in the database) are added to the active group as new peers, or a new group is created if no URLs matched existing peers.

## $REQ_GRP_007: CLI-Driven Group Resolution — Cross-Group Error
**Source:** ./specs/database.md (Section: "CLI-driven group resolution")

If CLI URLs match peers in different groups (per the config file), that is a config error.

## $REQ_GRP_008: Updated Group Written to Config
**Source:** ./specs/database.md (Section: "CLI-driven group resolution")

The updated group (with any newly added peers) is written back to the config file.

## $REQ_GRP_009: Config File Peer Group Structure
**Source:** ./specs/database.md (Section: "Config file structure")

The config file contains a `peer_groups` list. Each group has a `name` and a `peers` list. Each peer has a `name`, a `urls` list, and an optional `"canon": true` flag.

## $REQ_GRP_010: Fallback URLs in Config
**Source:** ./specs/database.md (Section: "Config file structure")

A peer's `urls` list can contain strings or objects with `"url"` plus optional per-URL settings (`"max-connections"`, `"connection-timeout"`). All URLs for a peer map to the same `peer_id`.

## $REQ_GRP_011: CLI URLs Are Single-Peer
**Source:** ./specs/concurrency.md (Section: "Fallback URLs")

On the CLI, each URL argument is a separate peer (single URL). Multiple fallback URLs per peer are a config-file feature.
