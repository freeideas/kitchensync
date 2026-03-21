# Peer Identity

Stable peer identity in the database and startup reconciliation against the config file.

## $REQ_PID_001: Stable Peer IDs
**Source:** ./specs/database.md (Section: "Peer Identity")

Each peer is assigned a stable integer ID. URLs are stored in a lookup table mapping to peer IDs. Snapshot rows are keyed by peer ID.

## $REQ_PID_002: Database Does Not Track Groups
**Source:** ./specs/database.md (Section: "Peer Identity")

The database stores peer identity (URL → peer ID mapping) only. Group structure lives entirely in the config file.

## $REQ_PID_003: Reconciliation Pass 1 — Recognize Known URLs
**Source:** ./specs/database.md (Section: "Startup reconciliation")

For each peer in the config, all URLs are normalized and looked up in `peer_url`. If any URL matches an existing `peer_id`, that config peer maps to that `peer_id`.

## $REQ_PID_004: Reconciliation Pass 1 — Merge Detection
**Source:** ./specs/database.md (Section: "Startup reconciliation")

If multiple URLs for a config peer match different `peer_id` values, those peers are being merged. The lowest `peer_id` is used.

## $REQ_PID_005: Reconciliation Pass 1 — New Peer Detection
**Source:** ./specs/database.md (Section: "Startup reconciliation")

If no URLs for a config peer match any existing `peer_id`, the peer is marked as new.

## $REQ_PID_006: Reconciliation Pass 1 — Ambiguous Identity Error
**Source:** ./specs/database.md (Section: "Startup reconciliation")

If two different config peers resolve to the same `peer_id`, that is a config error.

## $REQ_PID_007: Reconciliation Pass 2 — Create New Peers
**Source:** ./specs/database.md (Section: "Startup reconciliation")

New `peer` rows are created for peers marked as new in pass 1.

## $REQ_PID_008: Reconciliation Pass 2 — Migrate Snapshots on Merge
**Source:** ./specs/database.md (Section: "Startup reconciliation")

When peers are merged, snapshot rows are updated from the old `peer_id` to the surviving (lowest) `peer_id`. The now-empty old `peer` rows are deleted.

## $REQ_PID_009: Reconciliation Pass 2 — Rewrite URL Mappings
**Source:** ./specs/database.md (Section: "Startup reconciliation")

All `peer_url` rows are deleted and re-inserted from the config. The URL-to-peer mapping exactly mirrors the config file after reconciliation.

## $REQ_PID_010: URL Rename Preserves History
**Source:** ./specs/database.md (Section: "Why this works")

Renaming a peer's URL preserves all snapshot history because the `peer_id` does not change.

## $REQ_PID_011: Fallback URLs Share Peer ID
**Source:** ./specs/database.md (Section: "Peer Identity")

All fallback URLs for a single peer share one `peer_id` and one set of snapshot rows.
