//! Per-directory manifest: `<dir>/.kitchensync/manifest.txt`. See specs/manifest.md.

use std::collections::BTreeMap;

use crate::util::{format_micros, parse_time};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub is_dir: bool,
    pub mod_time: i64,
    pub byte_size: i64,
    pub last_seen: Option<i64>,
    pub deleted_time: Option<i64>,
    /// When KitchenSync itself put the current content here; None if the user did.
    pub placed: Option<i64>,
}

pub const NAME: &str = "manifest.txt";

fn enc(name: &str) -> String {
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

fn dec(s: &str) -> String {
    crate::util::decode_segment(s)
}

fn ts(v: Option<i64>) -> String {
    v.map(format_micros).unwrap_or_else(|| "-".to_string())
}

fn parse_ts(s: &str) -> Option<Option<i64>> {
    if s == "-" {
        Some(None)
    } else {
        parse_time(s).map(Some)
    }
}

/// Parse manifest text. Unparsable lines are ignored.
pub fn parse(text: &str) -> BTreeMap<String, Line> {
    let mut map = BTreeMap::new();
    for raw in text.lines() {
        let f: Vec<&str> = raw.split('\t').collect();
        if f.len() < 6 {
            continue;
        }
        let is_dir = match f[1] {
            "f" => false,
            "d" => true,
            _ => continue,
        };
        let (Some(mod_time), Ok(byte_size), Some(last_seen), Some(deleted_time)) =
            (parse_time(f[2]), f[3].parse::<i64>(), parse_ts(f[4]), parse_ts(f[5]))
        else {
            continue;
        };
        let placed = f.get(6).and_then(|v| parse_ts(v)).unwrap_or(None);
        map.insert(dec(f[0]), Line { is_dir, mod_time, byte_size, last_seen, deleted_time, placed });
    }
    map
}

/// Serialize, dropping tombstones older than `cutoff` (micros).
pub fn serialize(lines: &BTreeMap<String, Line>, tombstone_cutoff: i64) -> String {
    let mut out = String::new();
    for (name, l) in lines {
        if let Some(d) = l.deleted_time {
            if d < tombstone_cutoff {
                continue;
            }
        }
        out.push_str(&enc(name));
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
        out.push('\t');
        out.push_str(&ts(l.placed));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let mut m = BTreeMap::new();
        m.insert("a\tb.txt".to_string(), Line { is_dir: false, mod_time: 1_000_000, byte_size: 5, last_seen: Some(2_000_000), deleted_time: None, placed: None });
        m.insert("dir".to_string(), Line { is_dir: true, mod_time: 0, byte_size: -1, last_seen: None, deleted_time: Some(3_000_000), placed: Some(1) });
        let text = serialize(&m, 0);
        assert_eq!(parse(&text), m);
        assert!(text.starts_with("a%09b.txt\tf\t"));
        // Expired tombstone dropped.
        assert_eq!(parse(&serialize(&m, 4_000_000)).len(), 1);
    }
}
