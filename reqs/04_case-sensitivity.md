# 04_case-sensitivity: Filename case is preserved as the filesystem reports it

## Behavior

KitchenSync preserves filenames exactly as the underlying filesystem reports them; it neither normalizes case nor performs case-insensitive comparisons. Cross-platform syncs between case-sensitive and case-insensitive filesystems may collapse or duplicate names that differ only in case, in which case the displaced version is recoverable from BAK/. Derived from `./specs/sync.md` (`Case Sensitivity`).

## $REQ_IDs
- `04.41` — A file named `Photo.JPG` on a contributing peer is propagated to other peers using the same casing in name and extension.
- `04.42` — On a case-sensitive filesystem, two entries `Foo.txt` and `foo.txt` in the same directory are treated as distinct entries during sync.
- `04.43` — When syncing into a case-insensitive filesystem causes one of two case-differing files to be overwritten, the overwritten version is recoverable from BAK/.
