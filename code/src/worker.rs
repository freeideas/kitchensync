use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;

use crate::peer::Peer;
use crate::timestamp;

pub struct CopyJob {
    pub src_peer_name: String,
    pub src_path: String,
    pub dst_peer_name: String,
    pub dst_path: String,
}

pub struct WorkerPool {
    sender: Sender<CopyJob>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl WorkerPool {
    pub fn new(
        num_workers: usize,
        peers: Arc<std::collections::HashMap<String, Box<dyn Peer>>>,
    ) -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded::<CopyJob>();
        let mut handles = Vec::new();

        for _ in 0..num_workers {
            let rx = receiver.clone();
            let peers = peers.clone();
            let handle = std::thread::spawn(move || {
                worker_loop(rx, &peers);
            });
            handles.push(handle);
        }

        Self { sender, handles }
    }

    pub fn enqueue(&self, job: CopyJob) {
        self.sender.send(job).ok();
    }

    pub fn wait(self) {
        drop(self.sender);
        for handle in self.handles {
            handle.join().ok();
        }
    }
}

fn worker_loop(
    rx: Receiver<CopyJob>,
    peers: &std::collections::HashMap<String, Box<dyn Peer>>,
) {
    while let Ok(job) = rx.recv() {
        let src = match peers.get(&job.src_peer_name) {
            Some(p) => p,
            None => continue,
        };
        let dst = match peers.get(&job.dst_peer_name) {
            Some(p) => p,
            None => continue,
        };

        if let Err(e) = copy_file(src.as_ref(), &job.src_path, dst.as_ref(), &job.dst_path) {
            eprintln!(
                "Copy failed: {}:{} -> {}:{}: {}",
                job.src_peer_name, job.src_path, job.dst_peer_name, job.dst_path, e
            );
        }
    }
}

fn copy_file(
    src: &dyn Peer,
    src_path: &str,
    dst: &dyn Peer,
    dst_path: &str,
) -> Result<(), String> {
    let stamp = timestamp::now();
    let uuid = uuid::Uuid::new_v4().to_string();

    // Determine XFER and BACK paths
    let basename = std::path::Path::new(dst_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dst_path.to_string());
    let parent = parent_dir(dst_path);

    let xfer_path = format!(
        "{}/.kitchensync/XFER/{}/{}/{}",
        parent, stamp, uuid, basename
    );
    let back_path = format!("{}/.kitchensync/BACK/{}/{}", parent, stamp, basename);

    // Step 1: Transfer to XFER staging
    let mut reader = src
        .read_file(src_path)
        .map_err(|e| format!("Read error: {}", e))?;
    dst.write_file(&xfer_path, reader.as_mut())
        .map_err(|e| format!("Write XFER error: {}", e))?;

    // Step 2: Displace existing file to BACK
    if dst.stat(dst_path).is_ok() {
        dst.rename(dst_path, &back_path)
            .map_err(|e| format!("Displace to BACK error: {}", e))?;
    }

    // Step 3: Swap from XFER to final path
    dst.rename(&xfer_path, dst_path)
        .map_err(|e| format!("XFER swap error: {}", e))?;

    // Step 4: Clean up empty XFER directories (best effort)
    let xfer_uuid_dir = format!("{}/.kitchensync/XFER/{}/{}", parent, stamp, uuid);
    dst.delete_dir(&xfer_uuid_dir).ok();
    let xfer_stamp_dir = format!("{}/.kitchensync/XFER/{}", parent, stamp);
    dst.delete_dir(&xfer_stamp_dir).ok();

    Ok(())
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(pos) => path[..pos].to_string(),
        None => ".".to_string(),
    }
}
