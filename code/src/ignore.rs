//! Gitignore-style exclude patterns.
//!
//! Implements the pattern language described in `specs/sync.md`, section
//! "Excludes": the same rules as `.gitignore`, with no outside crates.
//!
//! A pattern is compiled once into a list of path segments, each segment a
//! small list of glob tokens, and matched against the slash-separated path an
//! entry has relative to the sync root. Patterns are kept in the order they
//! were added and the last one that matches decides the answer, so a later
//! negation (`!name`) can put back what an earlier pattern took away.
//!
//! # Deliberate simplifications
//!
//! * Matching is case-sensitive on every platform, like git's default.
//! * Git cannot re-include a file when one of its parent directories is
//!   excluded, and neither can this: [`IgnoreSet::is_excluded`] first walks the
//!   parent directories of the path top-down, and if any of them is excluded
//!   the entry is excluded too, whatever the patterns say about the entry
//!   itself. In practice the caller walks top-down and never asks about
//!   children of an excluded directory, so this parent check is normally a
//!   formality; it is here so that a single call is still right on its own.
//! * Git's `.gitignore` files take effect relative to the directory they sit
//!   in. Here every pattern is relative to the sync root, as the spec says.

/// One character-class item inside `[...]`.
#[derive(Clone, Debug)]
enum ClassItem {
    /// A single literal character.
    Ch(char),
    /// An inclusive range, e.g. `a-z`.
    Range(char, char),
}

/// A compiled `[...]` character class.
#[derive(Clone, Debug)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

impl Class {
    fn matches(&self, c: char) -> bool {
        let hit = self.items.iter().any(|item| match item {
            ClassItem::Ch(x) => *x == c,
            ClassItem::Range(a, b) => *a <= c && c <= *b,
        });
        hit != self.negated
    }
}

/// One glob token inside a single path segment.
#[derive(Clone, Debug)]
enum Tok {
    /// A literal character.
    Lit(char),
    /// `*`: any run of characters, never crossing a `/`.
    Any,
    /// `?`: exactly one character, never a `/`.
    One,
    /// `[...]`.
    Class(Class),
}

/// One path segment of a compiled pattern.
#[derive(Clone, Debug)]
enum Segment {
    /// A `**` segment: any number of whole path segments.
    AnyDepth,
    /// An ordinary segment, matched against one path component.
    Glob(Vec<Tok>),
}

/// One compiled pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    /// `true` for a `!pattern`, which puts entries back in.
    negated: bool,
    /// `true` for a `pattern/`, which only matches directories.
    dir_only: bool,
    /// `true` when the pattern held a `/` and so is pinned to the sync root.
    /// `false` means the pattern is matched against the entry's name alone,
    /// at any depth.
    anchored: bool,
    /// The compiled segments. For an unanchored pattern this is one segment.
    segs: Vec<Segment>,
    /// The line the pattern was written as, kept for messages and debugging.
    source: String,
}

impl Pattern {
    /// Does this pattern match the entry at `rel`, ignoring the parent
    /// directories of `rel`?
    fn matches(&self, parts: &[&str], is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        if parts.is_empty() {
            return false;
        }
        if self.anchored {
            match_segments(&self.segs, parts)
        } else {
            // One segment, matched against the entry's own name.
            match &self.segs[0] {
                Segment::AnyDepth => true,
                Segment::Glob(toks) => {
                    let name: Vec<char> = parts[parts.len() - 1].chars().collect();
                    match_tokens(toks, &name)
                }
            }
        }
    }

    /// The line this pattern was compiled from, with trailing spaces removed.
    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Does this pattern put entries back in rather than take them out?
    #[allow(dead_code)]
    pub fn is_negated(&self) -> bool {
        self.negated
    }
}

/// An ordered list of patterns; the last matching pattern wins.
#[derive(Default, Clone, Debug)]
pub struct IgnoreSet {
    patterns: Vec<Pattern>,
}

impl IgnoreSet {
    pub fn new() -> Self {
        IgnoreSet {
            patterns: Vec::new(),
        }
    }

    /// Built-in litter patterns: ".DS_Store", "._*", "Thumbs.db", "desktop.ini".
    pub fn builtin() -> Self {
        let mut set = IgnoreSet::new();
        for line in [".DS_Store", "._*", "Thumbs.db", "desktop.ini"] {
            // These are written here and are known good.
            set.add_line(line).expect("built-in pattern is valid");
        }
        set
    }

    /// Parse one pattern line (gitignore syntax). Returns `Err(String)` with a
    /// plain-language message for an invalid pattern (empty after trimming,
    /// contains NUL, unbalanced bracket). Comments (`#...`) and blank lines
    /// are accepted and ignored (`Ok` with nothing added).
    pub fn add_line(&mut self, line: &str) -> Result<(), String> {
        if let Some(p) = compile(line)? {
            self.patterns.push(p);
        }
        Ok(())
    }

    /// Parse a whole file's text, line by line. Errors mention the 1-based
    /// line number.
    pub fn add_text(&mut self, text: &str) -> Result<(), String> {
        for (i, raw) in text.split('\n').enumerate() {
            // Tolerate files saved with Windows line endings.
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            self.add_line(line)
                .map_err(|e| format!("line {}: {}", i + 1, e))?;
        }
        Ok(())
    }

    /// Append every pattern of `other` after this set's patterns.
    pub fn extend(&mut self, other: &IgnoreSet) {
        self.patterns.extend(other.patterns.iter().cloned());
    }

    /// Is the entry at slash-separated relative path `rel` (no leading slash,
    /// e.g. "movx/a.txt") excluded? `is_dir` says whether it is a directory.
    ///
    /// Directory patterns (trailing `/`) only match when `is_dir` is true.
    /// Returns true when the last matching pattern is a normal one, false when
    /// it is a negation or nothing matches. A path whose parent directory is
    /// excluded is excluded as well, and no negation can put it back, which is
    /// what git does too.
    pub fn is_excluded(&self, rel: &str, is_dir: bool) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return false;
        }
        // Parent directories first: once one is out, everything under it is out.
        for cut in 1..parts.len() {
            if self.decide(&parts[..cut], true) {
                return true;
            }
        }
        self.decide(&parts, is_dir)
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Number of compiled patterns in the set.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// The patterns, in the order they were added.
    #[allow(dead_code)]
    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    /// Last matching pattern wins; nothing matching means "keep it".
    fn decide(&self, parts: &[&str], is_dir: bool) -> bool {
        let mut excluded = false;
        for p in &self.patterns {
            if p.matches(parts, is_dir) {
                excluded = !p.negated;
            }
        }
        excluded
    }
}

/// Drop trailing spaces that were not escaped with a backslash.
fn trim_trailing_spaces(chars: &[char]) -> &[char] {
    // Walk forward so we know which characters a backslash is protecting.
    let mut end = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            // The backslash and whatever it protects both count.
            end = i + 2;
            i += 2;
        } else {
            if chars[i] != ' ' {
                end = i + 1;
            }
            i += 1;
        }
    }
    &chars[..end]
}

/// Compile one line. `Ok(None)` means the line was a comment or blank.
fn compile(line: &str) -> Result<Option<Pattern>, String> {
    if line.contains('\0') {
        return Err("pattern contains a NUL character".to_string());
    }
    let all: Vec<char> = line.chars().collect();
    let mut chars: &[char] = trim_trailing_spaces(&all);
    if chars.is_empty() {
        // Blank lines are simply skipped.
        return Ok(None);
    }
    if chars[0] == '#' {
        // Comment. Write `\#` to mean a file actually named "#...".
        return Ok(None);
    }
    let source: String = chars.iter().collect();

    let mut negated = false;
    if chars[0] == '!' {
        negated = true;
        chars = &chars[1..];
        if chars.is_empty() {
            return Err("pattern is empty after the leading '!'".to_string());
        }
    }

    // A trailing unescaped '/' means "directories only".
    let mut dir_only = false;
    while let Some((last, rest)) = chars.split_last() {
        if *last == '/' && !is_escaped(chars, chars.len() - 1) {
            dir_only = true;
            chars = rest;
        } else {
            break;
        }
    }
    if chars.is_empty() {
        return Err("pattern is empty apart from slashes".to_string());
    }

    let raw_segs = split_segments(chars);
    // A pattern holding a '/' anywhere (other than the trailing one) is pinned
    // to the sync root; a leading '/' says the same thing and adds an empty
    // first segment, which we drop.
    let anchored = raw_segs.len() > 1;
    let mut segs: Vec<Segment> = Vec::new();
    for raw in raw_segs {
        if raw.is_empty() {
            // From a leading '/' or a doubled '//': nothing to match.
            continue;
        }
        let seg = compile_segment(&raw)?;
        // Two `**` in a row say no more than one does.
        if matches!(seg, Segment::AnyDepth) && matches!(segs.last(), Some(Segment::AnyDepth)) {
            continue;
        }
        segs.push(seg);
    }
    if segs.is_empty() {
        return Err("pattern is empty after trimming".to_string());
    }
    if !anchored && segs.len() != 1 {
        // Cannot happen: without a '/' there is exactly one segment.
        return Err("pattern could not be understood".to_string());
    }

    Ok(Some(Pattern {
        negated,
        dir_only,
        anchored,
        segs,
        source,
    }))
}

/// Is the character at `idx` protected by an odd number of backslashes?
fn is_escaped(chars: &[char], idx: usize) -> bool {
    let mut back = 0usize;
    let mut i = idx;
    while i > 0 && chars[i - 1] == '\\' {
        back += 1;
        i -= 1;
    }
    back % 2 == 1
}

/// Split on unescaped slashes, keeping the escapes for the segment compiler.
fn split_segments(chars: &[char]) -> Vec<Vec<char>> {
    let mut out: Vec<Vec<char>> = Vec::new();
    let mut cur: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            cur.push(c);
            cur.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '/' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        i += 1;
    }
    out.push(cur);
    out
}

/// Compile one segment's text into either `**` or a token list.
fn compile_segment(chars: &[char]) -> Result<Segment, String> {
    // A segment that is nothing but `**` matches any number of segments. A
    // `**` with anything else beside it is just an ordinary `*`.
    if chars.len() == 2 && chars[0] == '*' && chars[1] == '*' {
        return Ok(Segment::AnyDepth);
    }
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                if i + 1 >= chars.len() {
                    // A lone trailing backslash stands for itself.
                    toks.push(Tok::Lit('\\'));
                    i += 1;
                } else {
                    toks.push(Tok::Lit(chars[i + 1]));
                    i += 2;
                }
            }
            '*' => {
                // Collapse runs of `*` into one, so `**` here acts like `*`.
                if !matches!(toks.last(), Some(Tok::Any)) {
                    toks.push(Tok::Any);
                }
                i += 1;
            }
            '?' => {
                toks.push(Tok::One);
                i += 1;
            }
            '[' => {
                let (class, next) = compile_class(chars, i)?;
                toks.push(Tok::Class(class));
                i = next;
            }
            _ => {
                toks.push(Tok::Lit(c));
                i += 1;
            }
        }
    }
    Ok(Segment::Glob(toks))
}

/// Compile a `[...]` class starting at `start` (which holds the `[`).
/// Returns the class and the index just past the closing `]`.
fn compile_class(chars: &[char], start: usize) -> Result<(Class, usize), String> {
    let mut i = start + 1;
    let mut negated = false;
    if i < chars.len() && (chars[i] == '!' || chars[i] == '^') {
        negated = true;
        i += 1;
    }
    let mut items: Vec<ClassItem> = Vec::new();
    // A ']' straight after the opening bracket is a plain ']'.
    let mut first = true;
    loop {
        if i >= chars.len() {
            return Err(
                "pattern has a '[' with no matching ']' (write '\\[' for a literal bracket)"
                    .to_string(),
            );
        }
        let c = chars[i];
        if c == ']' && !first {
            i += 1;
            break;
        }
        first = false;
        // One character, possibly escaped.
        let (lo, mut next) = if c == '\\' && i + 1 < chars.len() {
            (chars[i + 1], i + 2)
        } else {
            (c, i + 1)
        };
        // A range needs a '-' with something other than the closing ']' after it.
        if next + 1 < chars.len() && chars[next] == '-' && chars[next + 1] != ']' {
            let hi_at = next + 1;
            let (hi, after) = if chars[hi_at] == '\\' && hi_at + 1 < chars.len() {
                (chars[hi_at + 1], hi_at + 2)
            } else {
                (chars[hi_at], hi_at + 1)
            };
            items.push(ClassItem::Range(lo, hi));
            next = after;
        } else {
            items.push(ClassItem::Ch(lo));
        }
        i = next;
    }
    Ok((Class { negated, items }, i))
}

/// Match compiled segments against the path components.
fn match_segments(segs: &[Segment], parts: &[&str]) -> bool {
    match segs.first() {
        None => parts.is_empty(),
        Some(Segment::AnyDepth) => {
            if segs.len() == 1 {
                // A trailing `**` means "everything inside", so it needs at
                // least one segment to swallow: `logs/**` skips `logs` itself.
                return !parts.is_empty();
            }
            // Elsewhere `**` may stand for no segments at all.
            for cut in 0..=parts.len() {
                if match_segments(&segs[1..], &parts[cut..]) {
                    return true;
                }
            }
            false
        }
        Some(Segment::Glob(toks)) => {
            if parts.is_empty() {
                return false;
            }
            let head: Vec<char> = parts[0].chars().collect();
            match_tokens(toks, &head) && match_segments(&segs[1..], &parts[1..])
        }
    }
}

/// Match one segment's tokens against one path component.
fn match_tokens(toks: &[Tok], s: &[char]) -> bool {
    match toks.first() {
        None => s.is_empty(),
        Some(Tok::Any) => {
            // `*` never crosses a '/', and `s` is a single component already.
            for cut in 0..=s.len() {
                if match_tokens(&toks[1..], &s[cut..]) {
                    return true;
                }
            }
            false
        }
        Some(Tok::One) => !s.is_empty() && match_tokens(&toks[1..], &s[1..]),
        Some(Tok::Lit(c)) => !s.is_empty() && s[0] == *c && match_tokens(&toks[1..], &s[1..]),
        Some(Tok::Class(cl)) => {
            !s.is_empty() && cl.matches(s[0]) && match_tokens(&toks[1..], &s[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a set from a list of pattern lines, failing loudly on a bad one.
    fn set(lines: &[&str]) -> IgnoreSet {
        let mut s = IgnoreSet::new();
        for l in lines {
            s.add_line(l).unwrap_or_else(|e| panic!("{l:?}: {e}"));
        }
        s
    }

    fn file_excluded(lines: &[&str], rel: &str) -> bool {
        set(lines).is_excluded(rel, false)
    }

    fn dir_excluded(lines: &[&str], rel: &str) -> bool {
        set(lines).is_excluded(rel, true)
    }

    #[test]
    fn basename_matches_at_any_depth() {
        assert!(file_excluded(&[".DS_Store"], ".DS_Store"));
        assert!(file_excluded(&[".DS_Store"], "a/b/.DS_Store"));
        assert!(!file_excluded(&[".DS_Store"], "a/b/DS_Store"));
        assert!(!file_excluded(&[".DS_Store"], "a/.DS_Store2"));
    }

    #[test]
    fn star_does_not_cross_slash() {
        assert!(file_excluded(&["*.tmp"], "x/y.tmp"));
        assert!(file_excluded(&["*.tmp"], ".tmp"));
        assert!(!file_excluded(&["*.tmp"], "x/y.tmpx"));
        // '*' stops at a slash: this anchored pattern needs one segment.
        assert!(!file_excluded(&["/a*c"], "a/b/c"));
    }

    #[test]
    fn pattern_with_slash_is_anchored_at_root() {
        assert!(file_excluded(&["movx/quarantined"], "movx/quarantined"));
        assert!(!file_excluded(
            &["movx/quarantined"],
            "other/movx/quarantined"
        ));
    }

    #[test]
    fn leading_slash_is_the_same_as_a_slash_inside() {
        assert!(dir_excluded(&["/build"], "build"));
        assert!(!dir_excluded(&["/build"], "sub/build"));
        // Once "build" is out, everything under it is out too.
        assert!(file_excluded(&["/build"], "build/out.o"));
        assert!(!file_excluded(&["/build"], "sub/build/out.o"));
    }

    #[test]
    fn trailing_slash_means_directories_only() {
        assert!(dir_excluded(&["build/"], "build"));
        assert!(!file_excluded(&["build/"], "build"));
        assert!(dir_excluded(&["build/"], "a/build"));
        assert!(!file_excluded(&["build/"], "a/build"));
    }

    #[test]
    fn double_star_prefix_matches_every_directory() {
        assert!(dir_excluded(&["**/logs"], "logs"));
        assert!(dir_excluded(&["**/logs"], "a/b/logs"));
        assert!(!dir_excluded(&["**/logs"], "a/b/logsx"));
    }

    #[test]
    fn double_star_suffix_matches_only_what_is_inside() {
        assert!(file_excluded(&["logs/**"], "logs/x/y"));
        assert!(file_excluded(&["logs/**"], "logs/x"));
        assert!(!dir_excluded(&["logs/**"], "logs"));
        assert!(!file_excluded(&["logs/**"], "other/logs/x"));
    }

    #[test]
    fn double_star_in_the_middle_spans_any_number_of_segments() {
        assert!(file_excluded(&["a/**/b"], "a/b"));
        assert!(file_excluded(&["a/**/b"], "a/x/y/b"));
        assert!(!file_excluded(&["a/**/b"], "x/a/b"));
        assert!(!file_excluded(&["a/**/b"], "a/x/y/c"));
    }

    #[test]
    fn double_star_not_beside_slashes_acts_like_one_star() {
        assert!(file_excluded(&["a**b"], "axxb"));
        assert!(file_excluded(&["a**b"], "ab"));
        assert!(!file_excluded(&["/a**b"], "a/b"));
    }

    #[test]
    fn character_classes() {
        assert!(file_excluded(&["[abc]"], "b"));
        assert!(!file_excluded(&["[abc]"], "d"));
        assert!(file_excluded(&["[!a]"], "b"));
        assert!(!file_excluded(&["[!a]"], "a"));
        assert!(file_excluded(&["[^a]"], "b"));
        assert!(file_excluded(&["[a-c]x"], "bx"));
        assert!(!file_excluded(&["[a-c]x"], "dx"));
        assert!(!file_excluded(&["[a-c]x"], "Ax"));
        // A ']' first inside the class is a plain ']'.
        assert!(file_excluded(&["[]a]"], "]"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(file_excluded(&["?"], "a"));
        assert!(!file_excluded(&["?"], "ab"));
        assert!(!file_excluded(&["?"], ""));
        assert!(file_excluded(&["a?c"], "abc"));
        assert!(!file_excluded(&["/a?c"], "a/c"));
    }

    #[test]
    fn backslash_escapes_hash_bang_and_glob_characters() {
        assert!(file_excluded(&[r"\#x"], "#x"));
        assert!(!file_excluded(&[r"\#x"], "x"));
        assert!(file_excluded(&[r"\!x"], "!x"));
        // An escaped '*' is a plain '*', not a glob.
        assert!(file_excluded(&[r"a\*b"], "a*b"));
        assert!(!file_excluded(&[r"a\*b"], "axb"));
    }

    #[test]
    fn a_bare_hash_line_is_a_comment() {
        let s = set(&["# just a note", "   ", ""]);
        assert!(s.is_empty());
        assert!(!s.is_excluded("# just a note", false));
    }

    #[test]
    fn negation_puts_entries_back() {
        assert!(!file_excluded(&[".DS_Store", "!.DS_Store"], ".DS_Store"));
        assert!(file_excluded(&["!.DS_Store", ".DS_Store"], ".DS_Store"));
        assert!(file_excluded(&["*.log", "!keep.log"], "a.log"));
        assert!(!file_excluded(&["*.log", "!keep.log"], "keep.log"));
        assert!(!file_excluded(&["*.log", "!keep.log"], "a/keep.log"));
    }

    #[test]
    fn negation_cannot_reach_inside_an_excluded_directory() {
        // Git behaves the same way, and the walker never descends there anyway.
        assert!(file_excluded(
            &["build/", "!build/keep.txt"],
            "build/keep.txt"
        ));
    }

    #[test]
    fn builtin_covers_the_usual_operating_system_litter() {
        let s = IgnoreSet::builtin();
        assert!(s.is_excluded("._foo", false));
        assert!(s.is_excluded("a/b/._foo", false));
        assert!(s.is_excluded("Thumbs.db", false));
        assert!(s.is_excluded("x/desktop.ini", false));
        assert!(s.is_excluded(".DS_Store", false));
        assert!(!s.is_excluded("photo.jpg", false));
        assert!(!s.is_excluded("thumbs.db", false)); // case-sensitive, like git
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn builtin_can_be_turned_back_on_with_a_negation() {
        let mut s = IgnoreSet::builtin();
        s.add_line("!.DS_Store").unwrap();
        assert!(!s.is_excluded(".DS_Store", false));
        assert!(s.is_excluded("Thumbs.db", false));
    }

    #[test]
    fn add_text_reads_comments_blank_lines_and_trailing_spaces() {
        let mut s = IgnoreSet::new();
        s.add_text("# litter\n\n*.tmp   \n\n  \n# end\n").unwrap();
        assert_eq!(s.len(), 1);
        assert!(s.is_excluded("a/b.tmp", false));
        // The trailing spaces were dropped, so "b.tmp   " is not a match.
        assert!(!s.is_excluded("a/b.tmp   ", false));

        // An escaped trailing space is kept.
        let mut s2 = IgnoreSet::new();
        s2.add_text("keep\\ \n").unwrap();
        assert!(s2.is_excluded("keep ", false));
        assert!(!s2.is_excluded("keep", false));
    }

    #[test]
    fn add_text_reports_the_line_number() {
        let mut s = IgnoreSet::new();
        let err = s.add_text("ok\n# fine\n[oops\n").unwrap_err();
        assert!(err.starts_with("line 3:"), "{err}");
    }

    #[test]
    fn add_text_handles_windows_line_endings() {
        let mut s = IgnoreSet::new();
        s.add_text("*.tmp\r\n# note\r\n").unwrap();
        assert_eq!(s.len(), 1);
        assert!(s.is_excluded("x.tmp", false));
    }

    #[test]
    fn blank_lines_are_accepted_and_add_nothing() {
        let mut s = IgnoreSet::new();
        assert!(s.add_line("").is_ok());
        assert!(s.add_line("    ").is_ok());
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn invalid_patterns_are_reported() {
        let mut s = IgnoreSet::new();
        assert!(s.add_line("[").is_err());
        assert!(s.add_line("a[b-").is_err());
        assert!(s.add_line("!").is_err());
        assert!(s.add_line("/").is_err());
        assert!(s.add_line("a\0b").is_err());
        assert!(s.is_empty());
        // The message says something a person can act on.
        let msg = s.add_line("[abc").unwrap_err();
        assert!(msg.contains("'['"), "{msg}");
    }

    #[test]
    fn extend_appends_after_the_existing_patterns() {
        let mut base = IgnoreSet::builtin();
        let extra = set(&["!.DS_Store", "*.bak"]);
        base.extend(&extra);
        assert_eq!(base.len(), 6);
        assert!(!base.is_excluded(".DS_Store", false));
        assert!(base.is_excluded("x.bak", false));

        // The other set is untouched and order is preserved.
        assert_eq!(extra.len(), 2);
        assert_eq!(base.patterns()[4].source(), "!.DS_Store");
        assert!(base.patterns()[4].is_negated());
    }

    #[test]
    fn an_empty_set_excludes_nothing() {
        let s = IgnoreSet::new();
        assert!(s.is_empty());
        assert!(!s.is_excluded("anything/at/all", false));
        assert!(!s.is_excluded("", false));
    }

    #[test]
    fn everything_under_an_excluded_directory_is_excluded() {
        assert!(file_excluded(&["node_modules/"], "a/node_modules/x/y.js"));
        assert!(file_excluded(&["*.tmp"], "x.tmp/inside.txt"));
        assert!(!file_excluded(&["*.tmp"], "x.tmpx/inside.txt"));
    }

    #[test]
    fn empty_relative_path_is_never_excluded() {
        assert!(!file_excluded(&["*"], ""));
        assert!(!dir_excluded(&["*"], ""));
    }
}
