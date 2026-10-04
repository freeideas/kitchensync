//! Filesystem helpers shared by the walk and the copy workers: SWAP recovery,
//! displacement to BAK, the per-directory layout's conversion, recursive
//! delete, and listing retries. See specs/sync.md ("SWAP Directory",
//! "Displace to BAK") and specs/multi-tree-sync.md ("Reading A Directory").

use std::sync::Arc;

use crate::output;
use crate::transport::{join, Entry, ErrorKind, Result, Transport, TransportError};
use crate::util::{decode_segment, encode_segment, parse_time, system_to_micros};

use super::moves::MOVE_MIN;
use super::peer::{note_failure, Peer, META};

pub type PeerRef = Arc<Peer>;

pub fn meta_dir(dir: &str) -> String {
    join(dir, META)
}

pub fn swap_dir_for(target: &str) -> String {
    let parent = crate::util::parent_path(target);
    let base = crate::util::basename(target);
    join(&meta_dir(parent), &format!("SWAP/{}", encode_segment(base)))
}

pub fn exists(t: &dyn Transport, path: &str) -> Result<bool> {
    match t.stat(path) {
        Ok(_) => Ok(true),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(e),
    }
}

/// List a directory with up to `tries` total attempts.
pub fn list_with_retries(t: &dyn Transport, path: &str, tries: u32) -> Result<Vec<Entry>> {
    let mut last = TransportError::io("no attempts");
    for _ in 0..tries.max(1) {
        match t.list_dir(path) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Move `from` to BAK as a displacement of `rel`, journal it, and let the
/// move index follow it. `entry` describes what was at `rel` when known.
pub fn archive(peer: &Peer, from: &str, rel: &str, entry: Option<&Entry>) -> Result<String> {
    let _fs = peer.moves.lock_fs();
    let to = peer.move_to_bak(from, rel)?;
    let file = entry.filter(|e| !e.is_dir).map(|e| (e.byte_size, system_to_micros(e.mod_time)));
    peer.journal('X', rel, Some(&to), file.map(|f| f.0), file.map(|f| f.1));
    match entry {
        Some(e) if e.is_dir => {
            for (path, size, mt) in peer.history.files_under(rel) {
                if size >= MOVE_MIN {
                    let inside = format!("{to}{}", &path[rel.len()..]);
                    peer.journal('B', &path, Some(&inside), Some(size), Some(mt));
                    peer.moves.add(inside, size, mt);
                }
            }
            peer.moves.displaced(rel, &to);
        }
        Some(_) => {
            peer.moves.displaced(rel, &to);
            if let Some((size, mt)) = file {
                peer.moves.add(to.clone(), size, mt);
            }
        }
        None => peer.moves.displaced(rel, &to),
    }
    Ok(to)
}

/// Recover one SWAP directory for `<parent>/<basename>` on a peer.
pub fn recover_one_swap(peer: &Peer, parent: &str, basename: &str) -> Result<()> {
    let t = peer.t();
    let target = join(parent, basename);
    let swap = swap_dir_for(&target);
    let new = join(&swap, "new");
    let old = join(&swap, "old");
    let has_old = exists(t, &old)?;
    let has_new = exists(t, &new)?;
    let has_target = exists(t, &target)?;
    match (has_old, has_new, has_target) {
        (true, _, true) => {
            if has_new {
                remove_tree(t, &new)?;
            }
            archive(peer, &old, &target, None)?;
        }
        (true, true, false) => {
            t.rename(&new, &target)?;
            archive(peer, &old, &target, None)?;
        }
        (true, false, false) => {
            t.rename(&old, &target)?;
        }
        (false, true, true) => {
            remove_tree(t, &new)?;
        }
        (false, true, false) => {
            // Without `old` there is no sign the transfer into `new` finished:
            // it may be cut short. Putting it in place would make a partial
            // file the newest copy. Drop it; the next decision copies again.
            remove_tree(t, &new)?;
        }
        (false, false, _) => {}
    }
    let _ = delete_litter_dir(t, &swap);
    Ok(())
}

/// Recover every SWAP directory under `<dir>/.kitchensync/SWAP/` on a peer.
pub fn recover_swaps(peer: &Peer, dir: &str) -> Result<()> {
    let swap_root = join(&meta_dir(dir), "SWAP");
    let entries = match peer.t().list_dir(&swap_root) {
        Ok(v) => v,
        Err(e) if e.is_not_found() => return Ok(()),
        Err(e) => return Err(e),
    };
    for e in entries.iter().filter(|e| e.is_dir) {
        recover_one_swap(peer, dir, &decode_segment(&e.name))?;
    }
    let _ = delete_litter_dir(peer.t(), &swap_root);
    Ok(())
}

fn is_litter(e: &Entry) -> bool {
    !e.is_dir && (e.name.starts_with("._") || e.name == ".DS_Store")
}

/// Remove a directory that holds nothing but operating-system litter.
/// macOS writes `._<name>` AppleDouble files beside anything it touches on
/// exFAT/FAT drives (and Finder drops `.DS_Store`), so a directory that a
/// Mac has seen is never truly empty. Litter is deleted first; any other
/// entry is left alone and the directory delete fails as it would anyway.
pub fn delete_litter_dir(t: &dyn Transport, path: &str) -> Result<()> {
    let entries = match t.list_dir(path) {
        Ok(v) => v,
        Err(e) if e.is_not_found() => return Ok(()),
        Err(e) => return Err(e),
    };
    if entries.iter().all(is_litter) {
        for e in &entries {
            t.delete_file(&join(path, &e.name))?;
        }
    }
    t.delete_dir(path)
}

/// Recursively delete a file or directory tree.
pub fn remove_tree(t: &dyn Transport, path: &str) -> Result<()> {
    let st = match t.stat(path) {
        Ok(s) => s,
        Err(e) if e.is_not_found() => return Ok(()),
        Err(e) => return Err(e),
    };
    if !st.is_dir {
        return t.delete_file(path);
    }
    for child in t.list_dir(path)? {
        remove_tree(t, &join(path, &child.name))?;
    }
    t.delete_dir(path)
}

/// What a directory's own `.kitchensync` folder held (specs/multi-tree-sync.md,
/// "Reading A Directory").
#[derive(Default)]
pub struct MetaFolder {
    pub swap: bool,
    pub state: bool,
    pub manifest: Option<&'static str>,
}

pub fn inspect_meta(t: &dyn Transport, dir: &str) -> Result<MetaFolder> {
    let names: Vec<String> = match t.list_dir(&meta_dir(dir)) {
        Ok(v) => v.into_iter().map(|e| e.name).collect(),
        Err(e) if e.is_not_found() => Vec::new(),
        Err(e) => return Err(e),
    };
    let has = |n: &str| names.iter().any(|x| x == n);
    let manifest = ["manifest.txt", "manifest.txt.new", "manifest.txt.old"].into_iter().find(|n| has(n));
    Ok(MetaFolder { swap: has("SWAP"), state: has(crate::state::STATE), manifest })
}

/// Convert a directory's per-directory layout (specs/state.md,
/// "Per-directory manifests"): BAK entries move to the root's BAK, the
/// manifest files go, and emptied folders are removed.
pub fn convert_meta(peer: &Peer, dir: &str) {
    let t = peer.t();
    let meta = meta_dir(dir);
    let fail = |what: &str, e: &dyn std::fmt::Display| output::error(&format!("conversion of {} on {}: {}: {}", meta, peer.url, what, e));
    let bak = join(&meta, "BAK");
    if let Ok(stamps) = t.list_dir(&bak) {
        for s in stamps.iter().filter(|s| s.is_dir) {
            let from_dir = join(&bak, &s.name);
            let items = match t.list_dir(&from_dir) {
                Ok(v) => v,
                Err(e) => {
                    fail("listing", &e);
                    continue;
                }
            };
            let to_dir = super::peer::meta(&format!("BAK/{}/{}", s.name, dir));
            if !items.iter().all(is_litter) {
                if let Err(e) = peer.ensure_dir(&to_dir) {
                    fail("making BAK folder", &e);
                    continue;
                }
            }
            // Litter is not moved: macOS carries a `._` file along with its
            // file on exFAT, and any left behind is deleted with the folder.
            for it in items.iter().filter(|e| !is_litter(e)) {
                if let Err(e) = t.rename(&join(&from_dir, &it.name), &join(&to_dir, &it.name)) {
                    fail("moving BAK entry", &e);
                }
            }
            let _ = delete_litter_dir(t, &from_dir);
        }
        let _ = delete_litter_dir(t, &bak);
    }
    for name in ["manifest.txt", "manifest.txt.new", "manifest.txt.old"] {
        let p = join(&meta, name);
        if let Err(e) = t.delete_file(&p) {
            if !e.is_not_found() {
                fail("deleting manifest", &e);
            }
        }
    }
    let _ = delete_litter_dir(t, &meta);
}

/// Displace an entry to BAK. In dry-run nothing touches the peer. Returns
/// true on success.
pub fn displace(peer: &Peer, rel: &str, entry: Option<&Entry>, dry_run: bool) -> bool {
    if dry_run {
        return true;
    }
    match archive(peer, rel, rel, entry) {
        Ok(_) => true,
        Err(e) => {
            output::error(&format!("displacement failed for {} on {}: {}", rel, peer.url, e));
            note_failure();
            false
        }
    }
}

/// Delete expired `BAK/<ts>/` folders and journals at the root.
pub fn cleanup_root(peer: &Peer, keep_bak_days: u64) {
    let t = peer.t();
    let cutoff = crate::util::now_micros() - (keep_bak_days as i64) * 86_400 * 1_000_000;
    let bak = super::peer::meta("BAK");
    if let Ok(entries) = t.list_dir(&bak) {
        for e in entries {
            if parse_time(&e.name).is_some_and(|ts| ts < cutoff) {
                if let Err(err) = remove_tree(t, &join(&bak, &e.name)) {
                    output::error(&format!("cleanup failed for {}/{} on {}: {}", bak, e.name, peer.url, err));
                }
            }
        }
    }
    let journals = super::peer::meta("journal");
    if let Ok(entries) = t.list_dir(&journals) {
        for e in entries {
            let stamp = e.name.strip_suffix(".txt").unwrap_or(&e.name);
            if parse_time(stamp).is_some_and(|ts| ts < cutoff) {
                let _ = t.delete_file(&join(&journals, &e.name));
            }
        }
    }
}

/// Category name for diagnostics.
pub fn kind_name(e: &TransportError) -> &'static str {
    match e.kind {
        ErrorKind::NotFound => "not_found",
        ErrorKind::PermissionDenied => "permission_denied",
        ErrorKind::Io => "io_error",
    }
}
