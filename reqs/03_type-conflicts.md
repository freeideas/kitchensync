# 03_type-conflicts: Same path is a file on one peer and a directory on another

## Behavior

When peers disagree on whether a path is a file or a directory, the conflict is resolved by displacing the loser to BAK/ and propagating the winning type. With a canon peer, canon's type wins; without a canon peer (or when canon has no entry at that path), the file wins. Derived from `./specs/multi-tree-sync.md` (`Type Conflicts`).

## $REQ_IDs
- `03.41` — When canon has a file at a path and another peer has a directory there, that peer's directory (with its subtree) is displaced to BAK/ and the canon file is copied in its place.
- `03.42` — When canon has a directory at a path and another peer has a file there, that peer's file is displaced to BAK/ and the directory is created on that peer.
- `03.43` — Without a canon peer, when one contributing peer has a file at a path and another contributing peer has a directory, the file wins; the directory (with its subtree) is displaced to BAK/.
- `03.44` — After resolving a file-vs-directory conflict in favor of "file", the winning file across the file-holding peers is selected by the normal decision rules (newest wins, ties keep larger).
