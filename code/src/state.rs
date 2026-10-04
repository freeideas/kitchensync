//! The text formats kept in `<root>/.kitchensync/`: the state file, the
//! journal, and the per-directory manifests that are still read.
//! See specs/state.md.

use std::collections::BTreeMap;

use crate::util::{format_micros, parse_time};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub is_dir: bool,
    pub mod_time: i64,
    pub byte_size: i64,
    pub last_seen: Option<i64>,
    pub deleted_time: Option<i64>,
}

/// One directory's lines, by child name.
pub type DirLines = BTreeMap<String, Line>;
/// A peer's lines, by directory ("" is the sync root), then by child name.
pub type Tree = BTreeMap<String, DirLines>;

pub const STATE: &str = "state.txt";
/// Where format 2 keeps the lines: `state.txt` compressed with gzip.
/// `state.txt` itself then holds only its first line, which names the format.
pub const STATE_GZ: &str = "state.gz";

/// The format of the state file and journal this KitchenSync writes, and the
/// newest it reads (specs/state.md, "Format version").
pub const FORMAT: u32 = 2;

/// The format version a state file or journal declares on its `#` line.
/// A file that names no version is format 1, the first one.
pub fn version(text: &str) -> u32 {
    let Some(first) = text.lines().next() else { return 1 };
    let f: Vec<&str> = first.split('\t').collect();
    if f[0] != "#" {
        return 1;
    }
    f.get(1).and_then(|v| v.parse().ok()).unwrap_or(1)
}

/// The first line of a journal.
pub fn journal_header() -> String {
    format!("#\t{FORMAT}\n")
}
pub const LEGACY_MANIFEST: &str = "manifest.txt";

/// Percent-encode a name so tab, newline, carriage return and `%` never appear raw.
pub fn enc(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '\t' => out.push_str("%09"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            '%' => out.push_str("%25"),
            c => out.push(c),
        }
    }
    out
}

pub fn dec(s: &str) -> String {
    crate::util::decode_segment(s)
}

/// Encode a relative path segment by segment.
pub fn enc_path(rel: &str) -> String {
    rel.split('/').map(enc).collect::<Vec<_>>().join("/")
}

pub fn dec_path(s: &str) -> String {
    s.split('/').map(dec).collect::<Vec<_>>().join("/")
}

fn ts(v: Option<i64>) -> String {
    v.map(format_micros).unwrap_or_else(|| "-".to_string())
}

fn parse_ts(s: &str) -> Option<Option<i64>> {
    if s == "-" { Some(None) } else { parse_time(s).map(Some) }
}

/// `kind`, `mod_time`, `byte_size`, `last_seen`, `deleted_time`.
fn parse_fields(f: &[&str]) -> Option<Line> {
    let is_dir = match *f.first()? {
        "f" => false,
        "d" => true,
        _ => return None,
    };
    Some(Line { is_dir, mod_time: parse_time(f.get(1)?)?, byte_size: f.get(2)?.parse().ok()?, last_seen: parse_ts(f.get(3)?)?, deleted_time: parse_ts(f.get(4)?)? })
}

/// Names compare in NFC. When two lines collapse to one name, keep the live
/// line over a tombstone, then the one seen most recently.
fn insert_better(map: &mut DirLines, name: String, line: Line) {
    match map.get(&name) {
        Some(old) if (old.deleted_time.is_none(), old.last_seen) >= (line.deleted_time.is_none(), line.last_seen) => {}
        _ => {
            map.insert(name, line);
        }
    }
}

fn split(path: &str) -> (&str, &str) {
    (crate::util::parent_path(path), crate::util::basename(path))
}

/// Parse a state file: when it was written, and its lines.
pub fn parse_state(text: &str) -> (Option<i64>, Tree) {
    let mut written = None;
    let mut tree = Tree::new();
    for raw in text.lines() {
        let f: Vec<&str> = raw.split('\t').collect();
        if f[0] == "#" {
            if written.is_none() {
                // `#`, the format version, the time written.
                written = f.get(2).or(f.get(1)).and_then(|v| parse_time(v));
            }
            continue;
        }
        if f[0].is_empty() || f[0].starts_with('#') {
            continue;
        }
        let Some(line) = parse_fields(&f[1..]) else { continue };
        let path = crate::transport::normalize::nfc(&dec_path(f[0]));
        let (dir, name) = split(&path);
        insert_better(tree.entry(dir.to_string()).or_default(), name.to_string(), line);
    }
    (written, tree)
}

fn push_line(out: &mut String, path: &str, l: &Line) {
    out.push_str(&enc_path(path));
    out.push('\t');
    out.push(if l.is_dir { 'd' } else { 'f' });
    out.push('\t');
    out.push_str(&format_micros(l.mod_time));
    out.push('\t');
    out.push_str(&l.byte_size.to_string());
    out.push('\t');
    out.push_str(&ts(l.last_seen));
    out.push('\t');
    out.push_str(&ts(l.deleted_time));
    out.push('\n');
}

/// The entry lines of a state file, sorted by path, dropping tombstones older
/// than `tombstone_cutoff`. `prefix` is cut from every directory (for writing
/// a nested root's own file), and only directories under it are included.
pub fn serialize_body(tree: &Tree, tombstone_cutoff: i64, prefix: &str) -> String {
    let mut rows: Vec<(String, &Line)> = Vec::new();
    for (dir, lines) in tree {
        let rel_dir = if prefix.is_empty() {
            dir.as_str()
        } else if dir == prefix {
            ""
        } else if let Some(rest) = dir.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')) {
            rest
        } else {
            continue;
        };
        for (name, l) in lines {
            if l.deleted_time.is_some_and(|d| d < tombstone_cutoff) {
                continue;
            }
            rows.push((crate::transport::join(rel_dir, name), l));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (path, l) in rows {
        push_line(&mut out, &path, l);
    }
    out
}

/// The marker `state.txt` of format 2: just its first line.
pub fn marker(written: i64) -> String {
    format!("#\t{FORMAT}\t{}\n", format_micros(written))
}

pub fn gzip(text: &str) -> Vec<u8> {
    use std::io::Write;
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(text.as_bytes()).expect("compressing into memory");
    z.finish().expect("compressing into memory")
}

pub fn gunzip(bytes: &[u8]) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes).read_to_string(&mut text)?;
    Ok(text)
}

pub fn with_header(written: i64, body: &str) -> String {
    format!("#\t{FORMAT}\t{}\n{}", format_micros(written), body)
}

/// Parse a per-directory `manifest.txt` (name, kind, mod_time, byte_size,
/// last_seen, deleted_time, then ignored fields).
pub fn parse_manifest(text: &str) -> DirLines {
    let mut map = DirLines::new();
    for raw in text.lines() {
        let f: Vec<&str> = raw.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let Some(line) = parse_fields(&f[1..]) else { continue };
        insert_better(&mut map, crate::transport::normalize::nfc(&dec(f[0])), line);
    }
    map
}

/// One journal line (specs/state.md, "Journal").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JLine {
    pub ts: i64,
    pub op: char,
    pub path: String,
    pub other: Option<String>,
    pub byte_size: Option<i64>,
    pub mod_time: Option<i64>,
}

impl JLine {
    pub fn format(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            format_micros(self.ts),
            self.op,
            enc_path(&self.path),
            self.other.as_deref().map(enc_path).unwrap_or_else(|| "-".into()),
            self.byte_size.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            ts(self.mod_time)
        )
    }
}

/// Parse journal text; lines that do not parse are skipped.
pub fn parse_journal(text: &str) -> Vec<JLine> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let f: Vec<&str> = raw.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let (Some(ts), Some(op)) = (parse_time(f[0]), f[1].chars().next().filter(|_| f[1].len() == 1)) else { continue };
        if !"XBCDM".contains(op) || f[2].is_empty() {
            continue;
        }
        let other = (f[3] != "-").then(|| dec_path(f[3]));
        let byte_size = if f[4] == "-" { None } else { match f[4].parse() { Ok(v) => Some(v), Err(_) => continue } };
        let Some(mod_time) = parse_ts(f[5]) else { continue };
        out.push(JLine { ts, op, path: dec_path(f[2]), other, byte_size, mod_time });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(is_dir: bool, deleted: Option<i64>) -> Line {
        Line { is_dir, mod_time: 1_000_000, byte_size: if is_dir { -1 } else { 5 }, last_seen: Some(2_000_000), deleted_time: deleted }
    }

    #[test]
    fn state_roundtrip() {
        let mut tree = Tree::new();
        tree.entry("".into()).or_default().insert("a\tb.txt".into(), line(false, None));
        tree.entry("".into()).or_default().insert("dir".into(), line(true, None));
        tree.entry("dir".into()).or_default().insert("x%y".into(), line(false, Some(3_000_000)));
        let text = with_header(9_000_000, &serialize_body(&tree, 0, ""));
        assert!(text.starts_with("#\t2\t1970-01-01_00-00-09_000000Z\na%09b.txt\tf\t"));
        assert_eq!(version(&text), 2);
        assert_eq!(gunzip(&gzip(&text)).unwrap(), text);
        assert_eq!(version(&marker(1)), 2);
        assert_eq!(version("#\t7\t1970-01-01_00-00-09_000000Z\n"), 7);
        assert_eq!(version("#\t1970-01-01_00-00-09_000000Z\na\tf\n"), 1, "no version named: the first format");
        assert_eq!(version("a\tf\n"), 1);
        assert_eq!(version(&journal_header()), FORMAT);
        let (written, back) = parse_state(&text);
        assert_eq!(written, Some(9_000_000));
        assert_eq!(back, tree);
        // Expired tombstone dropped.
        let (_, back) = parse_state(&serialize_body(&tree, 4_000_000, ""));
        assert!(back.get("dir").is_none_or(|d| d.is_empty()));
        // A nested root's file holds only its subtree, relative to it.
        assert_eq!(serialize_body(&tree, 0, "dir"), "x%25y\tf\t1970-01-01_00-00-01_000000Z\t5\t1970-01-01_00-00-02_000000Z\t1970-01-01_00-00-03_000000Z\n");
    }

    #[test]
    fn paths_are_nfc() {
        let nfd = "cafe\u{301}.txt";
        let nfc = "caf\u{e9}.txt";
        let text = format!(
            "d/{nfd}\tf\t2024-01-01_10-00-00_000000Z\t5\t2024-01-02_10-00-00_000000Z\t-\n\
             d/{nfc}\tf\t2024-01-01_10-00-00_000000Z\t5\t2024-01-01_10-00-00_000000Z\t2024-01-01_10-00-00_000000Z\n"
        );
        let (_, tree) = parse_state(&text);
        assert_eq!(tree["d"].len(), 1);
        assert!(tree["d"][nfc].deleted_time.is_none());
    }

    #[test]
    fn manifest_lines_are_read() {
        let m = parse_manifest("gone.txt\tf\t2024-01-01_10-00-00_000000Z\t5\t2024-02-01_10-00-00_000000Z\t-\t-\nbad line\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m["gone.txt"].byte_size, 5);
    }

    #[test]
    fn journal_roundtrip() {
        let j = JLine { ts: 5, op: 'X', path: "a/b\tc".into(), other: Some(".kitchensync/BAK/t/a/b\tc".into()), byte_size: Some(7), mod_time: Some(9) };
        let d = JLine { ts: 6, op: 'D', path: "a".into(), other: None, byte_size: None, mod_time: None };
        let text = format!("{}{}cut short\t", j.format(), d.format());
        assert_eq!(parse_journal(&text), vec![j, d]);
    }
}
