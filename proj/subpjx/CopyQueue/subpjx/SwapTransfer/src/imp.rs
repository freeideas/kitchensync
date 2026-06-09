use std::sync::Arc;
use std::time::SystemTime;
use crate::api::*;

const CHUNK: usize = 65536;

struct SwapTransferImpl;

fn split(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent, name)
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn swap_dir(target: &str) -> String {
    let (parent, base) = split(target);
    join(&join(parent, ".kitchensync/SWAP"), &percent_encode(base))
}

fn stream_inner(
    src: &dyn Fs,
    rh: &ReadHandle,
    dst: &dyn Fs,
    wh: &WriteHandle,
) -> Result<(), FsError> {
    loop {
        match src.read(rh, CHUNK)? {
            Some(chunk) => dst.write(wh, &chunk)?,
            None => return Ok(()),
        }
    }
}

fn stream_copy(
    src: &dyn Fs,
    src_path: &str,
    dst: &dyn Fs,
    dst_path: &str,
) -> Result<(), FsError> {
    let rh = src.open_read(src_path)?;
    let wh = match dst.open_write(dst_path) {
        Ok(h) => h,
        Err(e) => {
            let _ = src.close_read(rh);
            return Err(e);
        }
    };
    let res = stream_inner(src, &rh, dst, &wh);
    let r1 = src.close_read(rh);
    let r2 = dst.close_write(wh);
    res.and(r1).and(r2)
}

fn ensure_parent(fs: &dyn Fs, path: &str) {
    let (parent, _) = split(path);
    if !parent.is_empty() {
        let _ = fs.create_dir(parent);
    }
}

impl SwapTransfer for SwapTransferImpl {
    fn transfer(
        &self,
        src: &dyn Fs,
        src_path: &str,
        dst: &dyn Fs,
        dst_path: &str,
        mod_time: SystemTime,
        _tmp_dir: &str,
        bak_dest: &str,
        dry_run: bool,
    ) -> TransferOutcome {
        if dry_run {
            // Exercise copy machinery by reading source (024.5); mutate nothing.
            if let Ok(rh) = src.open_read(src_path) {
                loop {
                    match src.read(&rh, CHUNK) {
                        Ok(Some(_)) => {}
                        _ => break,
                    }
                }
                let _ = src.close_read(rh);
            }
            return TransferOutcome::Done;
        }

        let sdir = swap_dir(dst_path);
        let swap_new = format!("{}/new", sdir);
        let swap_old = format!("{}/old", sdir);

        // Step 1: recover any existing SWAP for this target (019.8).
        if !self.recover(dst, dst_path, bak_dest, false) {
            return TransferOutcome::Failed;
        }

        // Step 2: write source content to swap_new (019.1, 020.13-15).
        // Try native copy first; fall back to streaming.
        let wrote = dst
            .native_copy(src, src_path, &swap_new)
            .or_else(|_| stream_copy(src, src_path, dst, &swap_new));
        if wrote.is_err() {
            // Failure before old exists: delete staged new (019.9).
            let _ = dst.delete_file(&swap_new);
            let _ = dst.delete_dir(&sdir);
            return TransferOutcome::Failed;
        }

        // Step 3: move existing target to swap_old (019.2).
        let old_created = match dst.exists(dst_path) {
            Ok(true) => {
                if dst.rename(dst_path, &swap_old).is_err() {
                    // Leave original in place (019.10); clean up staged new; skip (019.11).
                    let _ = dst.delete_file(&swap_new);
                    let _ = dst.delete_dir(&sdir);
                    return TransferOutcome::Skipped;
                }
                true
            }
            Ok(false) => false,
            Err(_) => {
                let _ = dst.delete_file(&swap_new);
                let _ = dst.delete_dir(&sdir);
                return TransferOutcome::Failed;
            }
        };

        // Step 4: rename swap_new to final target path (019.3).
        if dst.rename(&swap_new, dst_path).is_err() {
            if !old_created {
                // No old yet; delete staged new (019.9).
                let _ = dst.delete_file(&swap_new);
                let _ = dst.delete_dir(&sdir);
            }
            // old exists: leave SWAP state for recovery (019.12).
            return TransferOutcome::Failed;
        }

        // Step 5: set destination modification time (019.4).
        if dst.set_mod_time(dst_path, mod_time).is_err() {
            if old_created {
                // Leave SWAP state for recovery (019.12).
            } else {
                // No SWAP state remains; clean empty SWAP dir.
                let _ = dst.delete_dir(&sdir);
            }
            return TransferOutcome::Failed;
        }

        // Step 6: archive swap_old to BAK (019.5).
        if old_created {
            ensure_parent(dst, bak_dest);
            if dst.rename(&swap_old, bak_dest).is_err() {
                // Leave old for recovery (019.13); replacement is complete.
                return TransferOutcome::Done;
            }
        }

        // Step 7: remove now-empty SWAP directory (019.6).
        let _ = dst.delete_dir(&sdir);

        TransferOutcome::Done
    }

    fn recover(&self, fs: &dyn Fs, target_path: &str, bak_dest: &str, dry_run: bool) -> bool {
        if dry_run {
            return true; // (019.21, 024.20)
        }

        let sdir = swap_dir(target_path);
        let swap_old = format!("{}/old", sdir);
        let swap_new = format!("{}/new", sdir);

        let old_exists = match fs.exists(&swap_old) {
            Ok(e) => e,
            Err(_) => return false,
        };
        let new_exists = match fs.exists(&swap_new) {
            Ok(e) => e,
            Err(_) => return false,
        };

        if !old_exists && !new_exists {
            return true;
        }

        let target_exists = match fs.exists(target_path) {
            Ok(e) => e,
            Err(_) => return false,
        };

        if old_exists && target_exists {
            // (019.15): move old to BAK, remove empty SWAP dir.
            ensure_parent(fs, bak_dest);
            if fs.rename(&swap_old, bak_dest).is_err() {
                return false;
            }
            let _ = fs.delete_dir(&sdir);
        } else if old_exists && new_exists && !target_exists {
            // (019.16): rename new to target, move old to BAK, remove SWAP dir.
            if fs.rename(&swap_new, target_path).is_err() {
                return false;
            }
            ensure_parent(fs, bak_dest);
            if fs.rename(&swap_old, bak_dest).is_err() {
                return false;
            }
            let _ = fs.delete_dir(&sdir);
        } else if old_exists && !new_exists && !target_exists {
            // (019.17): rename old back to target, remove SWAP dir.
            if fs.rename(&swap_old, target_path).is_err() {
                return false;
            }
            let _ = fs.delete_dir(&sdir);
        } else if !old_exists && new_exists && target_exists {
            // (019.18): delete new, remove SWAP dir.
            if fs.delete_file(&swap_new).is_err() {
                return false;
            }
            let _ = fs.delete_dir(&sdir);
        } else {
            // (019.19): !old+new+!target → rename new to target, remove SWAP dir.
            if fs.rename(&swap_new, target_path).is_err() {
                return false;
            }
            let _ = fs.delete_dir(&sdir);
        }

        true
    }
}

pub fn new() -> std::sync::Arc<dyn SwapTransfer> {
    Arc::new(SwapTransferImpl)
}
