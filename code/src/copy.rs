use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{bounded, Receiver, Sender};

use crate::database::Database;
use crate::hash;
use crate::logging::Logger;
use crate::peer::{PeerError, PeerFs};
use crate::pool::ConnectedPeer;
use crate::timestamp;

const CHANNEL_BOUND: usize = 16;

/// A file copy task to be executed.
pub struct CopyTask {
    pub src_peer: String,
    pub dst_peer: String,
    pub path: String,
    pub src_mod_time: String,
}

/// Execute all queued copies concurrently, respecting connection pool limits.
pub fn execute_copies(
    tasks: Vec<CopyTask>,
    peers: &[Arc<ConnectedPeer>],
    db: &Arc<Database>,
    logger: &Arc<Logger>,
    sync_stamp: &str,
) {
    if tasks.is_empty() {
        return;
    }

    let sync_stamp = sync_stamp.to_string();

    thread::scope(|s| {
        let mut handles = Vec::new();
        for task in tasks {
            let peers = peers;
            let db = db.clone();
            let logger = logger.clone();
            let sync_stamp = sync_stamp.clone();

            let src_peer = find_peer(peers, &task.src_peer);
            let dst_peer = find_peer(peers, &task.dst_peer);

            if src_peer.is_none() || dst_peer.is_none() {
                logger.error(&format!("Copy skipped {}: peer not found", task.path));
                continue;
            }

            let src = src_peer.unwrap().clone();
            let dst = dst_peer.unwrap().clone();

            let handle = s.spawn(move || {
                execute_single_copy(&task, &src, &dst, &db, &logger, &sync_stamp);
            });
            handles.push(handle);
        }
        for h in handles {
            h.join().ok();
        }
    });
}

fn find_peer<'a>(peers: &'a [Arc<ConnectedPeer>], name: &str) -> Option<&'a Arc<ConnectedPeer>> {
    peers.iter().find(|p| p.name == name)
}

fn execute_single_copy(
    task: &CopyTask,
    src: &ConnectedPeer,
    dst: &ConnectedPeer,
    db: &Database,
    logger: &Logger,
    sync_stamp: &str,
) {
    let xfer_ts = timestamp::now();
    let xfer_uuid = uuid::Uuid::new_v4().to_string();
    let basename = hash::basename(&task.path);
    let parent = path_parent(&task.path);

    let xfer_dir = format!(
        "{}/.kitchensync/XFER/{}/{}",
        if parent.is_empty() { "." } else { &parent },
        xfer_ts,
        xfer_uuid
    );
    let xfer_path = format!("{}/{}", xfer_dir, basename);

    // Acquire connections from both pools
    let src_guard = match src.pool.acquire() {
        Ok(g) => g,
        Err(e) => {
            logger.error(&format!("Copy failed {} (src pool): {}", task.path, e));
            return;
        }
    };
    let dst_guard = match dst.pool.acquire() {
        Ok(g) => g,
        Err(e) => {
            logger.error(&format!("Copy failed {} (dst pool): {}", task.path, e));
            return;
        }
    };

    // Step 1: Pipelined transfer to XFER staging
    let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = bounded(CHANNEL_BOUND);

    let transfer_result = thread::scope(|s| {
        let path = task.path.clone();
        let xfer_path_clone = xfer_path.clone();

        // Reader task: read from source, push chunks to channel
        let reader_handle = s.spawn(move || -> Result<(), PeerError> {
            let mut channel_writer = ChannelWriter { tx };
            src_guard.conn().read_file_to(&path, &mut channel_writer)?;
            drop(channel_writer);
            Ok(())
        });

        // Writer task: pull chunks from channel, write to destination XFER
        let writer_handle = s.spawn(move || -> Result<(), PeerError> {
            let mut channel_reader = ChannelReader {
                rx,
                buf: Vec::new(),
                pos: 0,
            };
            dst_guard.conn().write_file_from(&xfer_path_clone, &mut channel_reader)?;
            Ok(())
        });

        let read_result = reader_handle
            .join()
            .unwrap_or(Err(PeerError::IoError("reader panicked".into())));
        let write_result = writer_handle
            .join()
            .unwrap_or(Err(PeerError::IoError("writer panicked".into())));

        if let Err(e) = read_result {
            return Err(e);
        }
        if let Err(e) = write_result {
            return Err(e);
        }
        Ok(())
    });

    if let Err(e) = transfer_result {
        logger.error(&format!("Transfer failed {}: {}", task.path, e));
        // Clean up XFER staging
        if let Ok(g) = dst.pool.acquire() {
            let _ = g.conn().delete_file(&xfer_path);
            let _ = cleanup_empty_dirs(g.conn(), &xfer_dir);
        }
        return;
    }

    // Steps 2-5: displace existing, swap, set mod_time, cleanup
    if let Ok(g) = dst.pool.acquire() {
        // Step 2: If destination already has a file, displace to BACK
        if let Ok(Some(_)) = g.conn().stat(&task.path) {
            let back_ts = timestamp::now();
            let back_dir = format!(
                "{}/.kitchensync/BACK/{}",
                if parent.is_empty() { "." } else { &parent },
                back_ts
            );
            let back_path = format!("{}/{}", back_dir, basename);
            if let Err(e) = g.conn().rename(&task.path, &back_path) {
                logger.error(&format!("Displacement failed {} -> BACK: {}", task.path, e));
                let _ = g.conn().delete_file(&xfer_path);
                let _ = cleanup_empty_dirs(g.conn(), &xfer_dir);
                return;
            }
        }

        // Step 3: Swap — rename from XFER to final path
        if let Err(e) = g.conn().rename(&xfer_path, &task.path) {
            logger.error(&format!("Swap failed {} from XFER: {}", task.path, e));
            return;
        }

        // Step 4: Set mod_time
        if let Err(e) = g.conn().set_mod_time(&task.path, &task.src_mod_time) {
            logger.debug(&format!("set_mod_time warning {}: {}", task.path, e));
        }

        // Step 5: Clean up empty XFER directories
        let _ = cleanup_empty_dirs(g.conn(), &xfer_dir);
    }

    // Update snapshot: set last_seen after completed copy
    let id = hash::path_hash(&task.path);
    db.set_last_seen(&id, &task.dst_peer, sync_stamp);

    logger.trace(&format!(
        "Copy completed: {} -> {} for {}",
        task.src_peer, task.dst_peer, task.path
    ));
}

/// Displace an entry to BACK/ directory. Used inline during traversal.
pub fn displace_to_back(
    conn: &dyn PeerFs,
    path: &str,
    _logger: &Logger,
) -> Result<(), PeerError> {
    let basename = hash::basename(path);
    let parent = path_parent(path);
    let back_ts = timestamp::now();
    let back_dir = format!(
        "{}/.kitchensync/BACK/{}",
        if parent.is_empty() { "." } else { &parent },
        back_ts
    );
    let back_path = format!("{}/{}", back_dir, basename);
    conn.rename(path, &back_path)?;
    Ok(())
}

fn path_parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => trimmed[..i].to_string(),
        None => String::new(),
    }
}

fn cleanup_empty_dirs(conn: &dyn PeerFs, dir: &str) -> Result<(), PeerError> {
    let _ = conn.delete_dir(dir);
    let parent = path_parent(dir);
    if !parent.is_empty() {
        let _ = conn.delete_dir(&parent);
    }
    Ok(())
}

struct ChannelWriter {
    tx: Sender<Vec<u8>>,
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tx
            .send(buf.to_vec())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "channel closed"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct ChannelReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos < self.buf.len() {
            let n = std::cmp::min(buf.len(), self.buf.len() - self.pos);
            buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }

        match self.rx.recv() {
            Ok(chunk) => {
                let n = std::cmp::min(buf.len(), chunk.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                if n < chunk.len() {
                    self.buf = chunk;
                    self.pos = n;
                } else {
                    self.buf.clear();
                    self.pos = 0;
                }
                Ok(n)
            }
            Err(_) => Ok(0),
        }
    }
}
