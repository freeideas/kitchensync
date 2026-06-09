# 012_directory-and-type-decisions: Directory and type-conflict decisions

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Directory
Decisions" and "Type Conflicts".

It covers existence-based directory decisions, which never use mod_time: if any
contributing peer has the directory live, it should exist on all peers (create
where missing); if no contributing peer has it live but at least one has a
snapshot row for it and every such peer is now absent, it is deleted (displaced)
everywhere and the row is tombstoned; a contributing peer with no row has no
opinion and does not block deletion; if no contributing peer has it live or as a
row, subordinate peers that have it are displaced. Canon overrides as usual. It
covers type conflicts where a path is a file on one peer and a directory on
another: with canon present the canon type wins; without canon the file wins -
the conflicting directory is displaced on contributing peers and the winning
file is then chosen by the normal file rules and synced. A subordinate peer's
type does not influence the contributing decision and is conformed afterward.

The file-winner comparison invoked after a type conflict is resolved is
`011_decision-rules`. Directory rows tracked with `byte_size = -1` are described
in `013_snapshot-schema`; tombstoning and cascade are `017_snapshot-updates`.

## $REQ_IDs

- `012.1` -- When at least one contributing peer has a directory live in its listing, the directory is created on every active peer that lacks it.
- `012.2` -- Directory create and displace outcomes are unchanged by the directories' mod_time values.
- `012.3` -- When no contributing peer has a directory live, at least one contributing peer has a snapshot row for it, and every contributing peer that has a snapshot row for it is absent from the current listing, the directory is displaced to BAK/ on every peer that still has it.
- `012.4` -- A contributing peer with no snapshot row for a directory does not block displacement of that directory.
- `012.5` -- When no contributing peer has a directory live in its listing and no contributing peer has a snapshot row for it, subordinate peers that have the directory are displaced to BAK/.
- `012.6` -- When a canon peer has a directory, it is created on every peer that lacks it.
- `012.7` -- When a canon peer lacks a directory, it is displaced to BAK/ on every peer that has it.
- `012.8` -- When a path is a file on one peer and a directory on another and a canon peer has a file at that path, the conflicting directories are displaced to BAK/.
- `012.9` -- When a path is a file on one peer and a directory on another and a canon peer has a file at that path, the canon file is synced to every peer.
- `012.10` -- When a path is a file on one peer and a directory on another and a canon peer has a directory at that path, the conflicting files are displaced to BAK/.
- `012.11` -- When a path is a file on one peer and a directory on another and a canon peer has a directory at that path, the directory is created and synced on every peer.
- `012.12` -- When a path is a file on one peer and a directory on another and a canon peer lacks the path, the path is displaced to BAK/ on every peer that has it.
- `012.13` -- With no canon peer, when at least one contributing peer has a file and at least one contributing peer has a directory at the same path, the conflicting directory is displaced to BAK/ on each contributing peer that has it.
- `012.14` -- After the conflicting directory is displaced, the winning file is selected among the contributing file entries by the normal file decision rules and synced to all active peers.
- `012.15` -- A subordinate peer's file does not cause the file to win over a contributing peer's directory at the same path.
- `012.16` -- After the contributing type decision is made, a subordinate peer whose path has the wrong type is displaced to BAK/.
- `012.17` -- After the contributing type decision is made, a subordinate peer whose path had the wrong type is conformed to the decided type.
