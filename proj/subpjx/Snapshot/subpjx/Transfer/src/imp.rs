use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::api::*;

impl std::fmt::Debug for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransferError::NotFound => write!(f, "NotFound"),
            TransferError::PermissionDenied => write!(f, "PermissionDenied"),
            TransferError::Io => write!(f, "Io"),
        }
    }
}

const LIVE: &str = ".kitchensync/snapshot.db";
const SWAP_DIR: &str = ".kitchensync/SWAP/snapshot.db";
const SWAP_OLD: &str = ".kitchensync/SWAP/snapshot.db/old";
const SWAP_NEW: &str = ".kitchensync/SWAP/snapshot.db/new";

struct TransferImpl;

impl Transfer for TransferImpl {
    fn recover(&self, peer: &dyn PeerFiles, dry_run: bool) -> Result<(), TransferError> {
        if dry_run {
            return Ok(());
        }

        let has_old = peer.exists(SWAP_OLD)?;
        let has_new = peer.exists(SWAP_NEW)?;
        let has_live = peer.exists(LIVE)?;

        match (has_old, has_new, has_live) {
            // 016.14: old + live -> delete new if present, then delete old
            (true, _, true) => {
                if has_new {
                    peer.delete(SWAP_NEW)?;
                }
                peer.delete(SWAP_OLD)?;
            }
            // 016.15: old + new + no live -> rename new to live, delete old
            (true, true, false) => {
                peer.rename(SWAP_NEW, LIVE)?;
                peer.delete(SWAP_OLD)?;
            }
            // 016.16: old + no new + no live -> rename old to live
            (true, false, false) => {
                peer.rename(SWAP_OLD, LIVE)?;
            }
            // 016.17: no old + new + live -> delete new
            (false, true, true) => {
                peer.delete(SWAP_NEW)?;
            }
            // 016.18: no old + new + no live -> rename new to live
            (false, true, false) => {
                peer.rename(SWAP_NEW, LIVE)?;
            }
            // no old + no new -> nothing to recover
            (false, false, _) => {}
        }

        let _ = peer.delete_dir(SWAP_DIR);
        Ok(())
    }

    fn download(
        &self,
        peer: &dyn PeerFiles,
        tmp_dir: &Path,
        _dry_run: bool,
    ) -> Result<Downloaded, TransferError> {
        let local_path = fresh_local_path(tmp_dir);
        std::fs::create_dir_all(local_path.parent().unwrap())
            .map_err(|_| TransferError::Io)?;

        match peer.download(LIVE, &local_path) {
            Ok(()) => Ok(Downloaded { local_path, had_history: true }),
            Err(TransferError::NotFound) => {
                std::fs::File::create(&local_path).map_err(|_| TransferError::Io)?;
                Ok(Downloaded { local_path, had_history: false })
            }
            Err(e) => Err(e),
        }
    }

    fn upload(
        &self,
        peer: &dyn PeerFiles,
        local_db: &Path,
        dry_run: bool,
    ) -> Result<(), TransferError> {
        if dry_run {
            return Ok(());
        }

        // 016.8: write new database to SWAP/new
        peer.upload(local_db, SWAP_NEW)?;

        // 016.9: rename live to old when live exists
        let has_live = peer.exists(LIVE)?;
        if has_live {
            peer.rename(LIVE, SWAP_OLD)?;
        }

        // 016.10: rename new to live
        peer.rename(SWAP_NEW, LIVE)?;

        // 016.11: delete old now that new is live
        if has_live {
            peer.delete(SWAP_OLD)?;
        }

        let _ = peer.delete_dir(SWAP_DIR);
        Ok(())
    }
}

fn fresh_local_path(tmp_dir: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    tmp_dir
        .join(format!("{:016x}{:08x}", nanos, pid))
        .join("snapshot.db")
}

pub fn new() -> Arc<dyn Transfer> {
    Arc::new(TransferImpl)
}
