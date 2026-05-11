# Compile gitignore-style patterns into a path matcher

## Purpose
Turn the text content of zero or more `.syncignore` files (and the kitchensync built-in exclude set) into a reusable path matcher that decides, for any candidate path within a directory tree, whether it is ignored. The kitchensync multi-tree walk calls this component at every directory level: it accumulates ignore patterns from each ancestor directory and from the current directory's winning `.syncignore`, and asks the matcher to filter the union of entry names before per-entry decisions are made. This implements `ignore.md` §"Pattern Format", §"Hierarchy", §"Built-in Excludes", and the filtering step described in `multi-tree-sync.md` §"Resolution During the Multi-Tree Walk".

This component is pure — it does no filesystem I/O. The caller is responsible for reading `.syncignore` files; this component only operates on already-loaded pattern text.

## API surface

### Compiling pattern text

`compile_patterns(text: string) -> PatternSet`

Takes the raw text of a single `.syncignore` file and returns a structured `PatternSet`. The text is parsed line-by-line according to the gitignore grammar:

- Blank lines and lines beginning with `#` are ignored.
- Trailing whitespace is stripped unless escaped with backslash.
- A leading `!` makes the pattern a negation (re-includes a previously-excluded path).
- A leading `/` anchors the pattern to the directory where the `.syncignore` lives (not deeper).
- A trailing `/` restricts the pattern to directories only.
- `*` matches any run of characters except `/`. `?` matches one non-`/` character. `[abc]` is a character class.
- `**` is recognised as a directory-spanning wildcard in the documented gitignore positions: leading `**/` (match at any depth), trailing `/**` (match anything inside), and `/**/` (zero or more intermediate directories).
- All other characters match literally.

Malformed patterns (for example, an unclosed character class) cause the offending line to be skipped; the rest of the file still compiles. A `PatternSet` therefore always compiles successfully — diagnostics, if any, are returned as a separate list alongside it for the caller to log.

### Stacking ignore scopes

`Matcher` represents the ignore rules in effect at a particular directory during the walk. The kitchensync glue builds one Matcher per directory by stacking pattern sets from each ancestor along with the current level's set.

- `empty_matcher() -> Matcher` — the matcher in effect at the sync root with no ancestor rules.
- `push_scope(parent: Matcher, scope_dir: relative-path, set: PatternSet) -> Matcher` — produces a new Matcher that adds `set` at `scope_dir`. The scope directory is the path (relative to the sync root) of the directory whose `.syncignore` produced `set`. Anchored patterns (leading `/`) and directory-only patterns are interpreted relative to `scope_dir`. The parent matcher is not mutated.

The stacking order matters: deeper-scope patterns are evaluated after shallower ones, so a deeper `!pattern` can re-include something a shallower scope excluded, exactly as gitignore specifies.

### Querying the matcher

`is_ignored(m: Matcher, path: relative-path, is_dir: bool) -> bool`

Returns true if the given relative path (interpreted relative to the sync root, forward-slash-separated, no leading slash) is ignored by the rules in `m`. The `is_dir` flag distinguishes directories from files so that directory-only patterns (trailing `/`) match correctly. Negations are honoured in the normal gitignore manner: the last matching pattern across the entire stack wins.

### Built-in excludes

The matcher always treats the following as ignored, regardless of `.syncignore` contents:

- The path component `.kitchensync` at any depth (and everything inside it).
- Symbolic links and special files (devices, FIFOs, sockets). These do not appear in path text, so the caller must convey them through `is_ignored_entry`:

`is_ignored_entry(m: Matcher, path: relative-path, kind: EntryKind) -> bool` — like `is_ignored`, but takes an `EntryKind` of `file`, `dir`, `symlink`, or `special`. Symlinks and special files always return true. Files and directories defer to `is_ignored`.

The default exclude for `.git/` is treated as an implicit deepest-priority pattern at the sync root that any user pattern can negate; in particular, a `!.git/` line in a `.syncignore` re-includes it as gitignore semantics demand. The built-in `.kitchensync` exclude cannot be negated.

## Anchoring
- Pattern grammar (blank/comment lines, `*`, `?`, `[…]`, `**`, leading `/` anchoring, trailing `/` directory-only, leading `!` negation, "last match wins" precedence): the gitignore pattern syntax documented at git-scm.com, referenced by `ignore.md` §"Pattern Format".
- Hierarchical accumulation across nested directories with deeper patterns overriding shallower ones: gitignore's hierarchical semantics, referenced by `ignore.md` §"Hierarchy".
- Always-ignored `.kitchensync/`, symlinks, special files; default-but-overridable `.git/`: `ignore.md` §"Symlinks" and §"Built-in Excludes".
- `relative-path` (forward-slash, no leading slash) and `EntryKind` (file / dir / symlink / special): host-language primitives and the standard filesystem entry taxonomy.
