//! Filesystem helpers shared by the walk and the copy workers: SWAP recovery,
//! displacement to BAK, recursive delete, BAK/TMP cleanup, listing retries.
//! See specs/sync.md ("SWAP Directory", "BAK Directory") and
//! specs/multi-tree-sync.md ("SWAP Recovery During Traversal").

use std::sync::Arc;

use crate::output;
use crate::transport::{join, Entry, ErrorKind, Result, Transport, TransportError};
use crate::util::{decode_segment, encode_segment, now_string, parse_time};

use super::peer::{Peer, META};

pub fn meta_dir(dir: &str) -> String {
    join(dir, META)
}

pub fn swap_dir_for(target: &str) -> String {
    let parent = crate::util::parent_path(target);
    let base = crate::util::basename(target);
    join(&meta_dir(parent), &format!("SWAP/{}", encode_segment(base)))
}

fn exists(t: &dyn Transport, path: &str) -> Result<bool> {
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

/// Move `old` from a SWAP dir into BAK with a fresh timestamp directory.
fn archive_to_bak(t: &dyn Transport, parent: &str, basename: &str, from: &str) -> Result<()> {
    let bak = join(&meta_dir(parent), &format!("BAK/{}", now_string()));
    t.create_dir(&bak)?;
    t.rename(from, &join(&bak, basename))
}

/// Recover one SWAP directory for `<parent>/<basename>` on a peer.
pub fn recover_one_swap(t: &dyn Transport, parent: &str, basename: &str) -> Result<()> {
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
            archive_to_bak(t, parent, basename, &old)?;
        }
        (true, true, false) => {
            t.rename(&new, &target)?;
            archive_to_bak(t, parent, basename, &old)?;
        }
        (true, false, false) => {
            t.rename(&old, &target)?;
        }
        (false, true, true) => {
            remove_tree(t, &new)?;
        }
        (false, true, false) => {
            t.rename(&new, &target)?;
        }
        (false, false, _) => {}
    }
    let _ = t.delete_dir(&swap);
    Ok(())
}

/// Recover every SWAP directory under `<dir>/.kitchensync/SWAP/` on a peer.
/// At the root, the `snapshot.db` swap belongs to the snapshot upload and is
/// handled separately at startup.
pub fn recover_swaps(t: &dyn Transport, dir: &str) -> Result<()> {
    let swap_root = join(&meta_dir(dir), "SWAP");
    let entries = match t.list_dir(&swap_root) {
        Ok(v) => v,
        Err(e) if e.is_not_found() => return Ok(()),
        Err(e) => return Err(e),
    };
    for e in entries {
        if !e.is_dir {
            continue;
        }
        let basename = decode_segment(&e.name);
        recover_one_swap(t, dir, &basename)?;
    }
    let _ = t.delete_dir(&swap_root);
    Ok(())
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

/// Displace an entry to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`.
/// In dry-run nothing touches the peer. Returns true on success.
pub fn displace(peer: &Peer, path: &str, dry_run: bool) -> bool {
    if dry_run {
        return true;
    }
    let parent = crate::util::parent_path(path);
    let base = crate::util::basename(path);
    let t: &dyn Transport = peer.transport.as_ref();
    match archive_to_bak(t, parent, base, path) {
        Ok(()) => true,
        Err(e) => {
            output::error(&format!("displacement failed for {} on {}: {}", path, peer.url, e));
            super::peer::note_failure();
            false
        }
    }
}

/// Purge expired `<dir>/.kitchensync/BAK/<ts>/` entries.
pub fn cleanup_bak(peer: &Peer, dir: &str, keep_bak_days: u64) {
    let t: &dyn Transport = peer.transport.as_ref();
    let base = join(&meta_dir(dir), "BAK");
    let entries = match t.list_dir(&base) {
        Ok(v) => v,
        Err(_) => return,
    };
    let cutoff = crate::util::now_micros() - (keep_bak_days as i64) * 86_400 * 1_000_000;
    for e in entries {
        let Some(ts) = parse_time(&e.name) else { continue };
        if ts < cutoff {
            if let Err(err) = remove_tree(t, &join(&base, &e.name)) {
                output::error(&format!("cleanup failed for {}/{} on {}: {}", base, e.name, peer.url, err));
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

pub type PeerRef = Arc<Peer>;
