use std::collections::HashMap;
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::thread;
use crate::api::*;

const DEFAULT_SLOT_LIMIT: usize = 10;
const DEFAULT_TRY_LIMIT: u32 = 3;
const DEFAULT_BAK_SECS: u64 = 90 * 24 * 3600;
const DEFAULT_TMP_SECS: u64 = 2 * 24 * 3600;

struct ConfigState {
    slot_limit: usize,
    try_limit: u32,
    bak_retention: Duration,
    tmp_retention: Duration,
    dry_run: bool,
}

impl Default for ConfigState {
    fn default() -> Self {
        ConfigState {
            slot_limit: DEFAULT_SLOT_LIMIT,
            try_limit: DEFAULT_TRY_LIMIT,
            bak_retention: Duration::from_secs(DEFAULT_BAK_SECS),
            tmp_retention: Duration::from_secs(DEFAULT_TMP_SECS),
            dry_run: false,
        }
    }
}

struct SemInner {
    active: usize,
    limit: usize,
}

struct Semaphore {
    inner: Mutex<SemInner>,
    cond: Condvar,
}

impl Semaphore {
    fn new(limit: usize) -> Self {
        Self {
            inner: Mutex::new(SemInner { active: 0, limit }),
            cond: Condvar::new(),
        }
    }

    fn set_limit(&self, limit: usize) {
        self.inner.lock().unwrap().limit = limit;
        self.cond.notify_all();
    }

    fn acquire(&self) -> (usize, usize) {
        let mut g = self.inner.lock().unwrap();
        while g.active >= g.limit {
            g = self.cond.wait(g).unwrap();
        }
        g.active += 1;
        (g.active, g.limit)
    }

    fn release(&self) -> (usize, usize) {
        let mut g = self.inner.lock().unwrap();
        g.active = g.active.saturating_sub(1);
        self.cond.notify_one();
        (g.active, g.limit)
    }
}

struct Outstanding {
    count: Mutex<usize>,
    cond: Condvar,
}

impl Outstanding {
    fn new() -> Self {
        Self { count: Mutex::new(0), cond: Condvar::new() }
    }

    fn inc(&self) {
        *self.count.lock().unwrap() += 1;
    }

    fn dec(&self) {
        let mut n = self.count.lock().unwrap();
        *n -= 1;
        if *n == 0 {
            self.cond.notify_all();
        }
    }

    fn wait_zero(&self) {
        let mut n = self.count.lock().unwrap();
        while *n > 0 {
            n = self.cond.wait(n).unwrap();
        }
    }
}

struct TransportFsAdapter {
    transport: Arc<dyn transport::Transport>,
    peer_id: u64,
}

impl TransportFsAdapter {
    fn peer_handle(&self) -> transport::PeerHandle {
        transport::PeerHandle(self.peer_id)
    }
}

impl copyqueue_swaptransfer::Fs for TransportFsAdapter {
    fn open_read(
        &self,
        path: &str,
    ) -> Result<copyqueue_swaptransfer::ReadHandle, copyqueue_swaptransfer::FsError> {
        self.transport
            .open_read(&self.peer_handle(), path)
            .map(|h| copyqueue_swaptransfer::ReadHandle(h.0))
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn read(
        &self,
        handle: &copyqueue_swaptransfer::ReadHandle,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, copyqueue_swaptransfer::FsError> {
        self.transport
            .read(&transport::ReadHandle(handle.0), max_bytes)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn close_read(
        &self,
        handle: copyqueue_swaptransfer::ReadHandle,
    ) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .close_read(transport::ReadHandle(handle.0))
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn open_write(
        &self,
        path: &str,
    ) -> Result<copyqueue_swaptransfer::WriteHandle, copyqueue_swaptransfer::FsError> {
        self.transport
            .open_write(&self.peer_handle(), path)
            .map(|h| copyqueue_swaptransfer::WriteHandle(h.0))
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn write(
        &self,
        handle: &copyqueue_swaptransfer::WriteHandle,
        bytes: &[u8],
    ) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .write(&transport::WriteHandle(handle.0), bytes)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn close_write(
        &self,
        handle: copyqueue_swaptransfer::WriteHandle,
    ) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .close_write(transport::WriteHandle(handle.0))
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn create_dir(&self, path: &str) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .create_dir(&self.peer_handle(), path)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .rename(&self.peer_handle(), src, dst)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn delete_file(&self, path: &str) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .delete_file(&self.peer_handle(), path)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn delete_dir(&self, path: &str) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .delete_dir(&self.peer_handle(), path)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn exists(&self, path: &str) -> Result<bool, copyqueue_swaptransfer::FsError> {
        match self.transport.stat(&self.peer_handle(), path) {
            Ok(_) => Ok(true),
            Err(transport::TransportError::NotFound) => Ok(false),
            Err(_) => Err(copyqueue_swaptransfer::FsError),
        }
    }

    fn set_mod_time(
        &self,
        path: &str,
        time: SystemTime,
    ) -> Result<(), copyqueue_swaptransfer::FsError> {
        self.transport
            .set_mod_time(&self.peer_handle(), path, time)
            .map_err(|_| copyqueue_swaptransfer::FsError)
    }

    fn native_copy(
        &self,
        src: &dyn copyqueue_swaptransfer::Fs,
        src_path: &str,
        dst_path: &str,
    ) -> Result<(), copyqueue_swaptransfer::FsError> {
        // Attempt a local native copy when the src is also a TransportFsAdapter
        // backed by the same transport (i.e., both ends are local file:// peers).
        // If the transport doesn't support it, return Err so SwapTransfer falls
        // back to the streaming path.
        let _ = (src, src_path, dst_path);
        Err(copyqueue_swaptransfer::FsError)
    }
}

struct PeerFsAdapter {
    transport: Arc<dyn transport::Transport>,
    peer_id: u64,
}

impl PeerFsAdapter {
    fn peer_handle(&self) -> transport::PeerHandle {
        transport::PeerHandle(self.peer_id)
    }
}

impl copyqueue_stagingcleanup::PeerFs for PeerFsAdapter {
    fn list(&self, path: &str) -> Vec<String> {
        self.transport
            .list_dir(&self.peer_handle(), path)
            .map(|entries| entries.into_iter().map(|e| e.name).collect())
            .unwrap_or_default()
    }

    fn remove(&self, path: &str) {
        remove_recursive(self.transport.as_ref(), self.peer_id, path);
    }
}

fn remove_recursive(transport: &dyn transport::Transport, peer_id: u64, path: &str) {
    let handle = transport::PeerHandle(peer_id);
    match transport.stat(&handle, path) {
        Ok(stat) if stat.is_dir => {
            if let Ok(entries) = transport.list_dir(&handle, path) {
                for entry in entries {
                    remove_recursive(transport, peer_id, &format!("{}/{}", path, entry.name));
                }
            }
            let _ = transport.delete_dir(&handle, path);
        }
        Ok(_) => {
            let _ = transport.delete_file(&handle, path);
        }
        Err(_) => {}
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn now_ts() -> String {
    let us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    let total_secs = us / 1_000_000;
    let frac_us = (us % 1_000_000) as u32;
    let sec_of_day = (total_secs % 86_400) as u32;
    let day = total_secs / 86_400;
    let h = sec_of_day / 3_600;
    let m = (sec_of_day % 3_600) / 60;
    let s = sec_of_day % 60;
    let jd = day as i64 + 2_440_588;
    let l = jd + 68_569;
    let n = (4 * l) / 146_097;
    let l = l - (146_097 * n + 3) / 4;
    let i = (4_000 * (l + 1)) / 1_461_001;
    let l = l - (1_461 * i) / 4 + 31;
    let j = (80 * l) / 2_447;
    let d = l - (2_447 * j) / 80;
    let l = j / 11;
    let mo = j + 2 - 12 * l;
    let y = 100 * (n - 49) + i + l;
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{:06}Z", y, mo, d, h, m, s, frac_us)
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent, child)
    }
}

fn path_parent_name(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

fn open_peer_cached(
    transport: &Arc<dyn transport::Transport>,
    cache: &Mutex<HashMap<String, u64>>,
    peer_url: &str,
    dry_run: bool,
) -> Option<u64> {
    let mut guard = cache.lock().unwrap();
    if let Some(&id) = guard.get(peer_url) {
        return Some(id);
    }
    let conn = transport.open_peer(peer_url, &[], dry_run, Duration::from_secs(30))?;
    let id = conn.handle.0;
    guard.insert(peer_url.to_string(), id);
    Some(id)
}

fn run_copy(
    mut request: CopyRequest,
    copy_id: u64,
    try_limit: u32,
    slot_limit: usize,
    dry_run: bool,
    transport: Arc<dyn transport::Transport>,
    output: Arc<dyn output::Output>,
    swap_transfer: Arc<dyn copyqueue_swaptransfer::SwapTransfer>,
    peer_cache: Arc<Mutex<HashMap<String, u64>>>,
    semaphore: Arc<Semaphore>,
    outstanding: Arc<Outstanding>,
) {
    let on_success = request.on_success.take();
    let mut tries: u32 = 0;
    let mut copy_succeeded = false;
    loop {
        let (active, _) = semaphore.acquire();
        output.copy_slots(active, slot_limit);

        let src_id = open_peer_cached(&transport, &peer_cache, &request.src_peer, dry_run);
        let dst_id = open_peer_cached(&transport, &peer_cache, &request.dst_peer, dry_run);

        let outcome = match (src_id, dst_id) {
            (Some(si), Some(di)) => {
                let src_fs = TransportFsAdapter {
                    transport: Arc::clone(&transport),
                    peer_id: si,
                };
                let dst_fs = TransportFsAdapter {
                    transport: Arc::clone(&transport),
                    peer_id: di,
                };
                let (parent, basename) = path_parent_name(&request.dst_path);
                let ts = now_ts();
                let tmp_dir =
                    join_path(parent, &format!(".kitchensync/TMP/{}", copy_id));
                let bak_dest =
                    join_path(parent, &format!(".kitchensync/BAK/{}/{}", ts, basename));
                swap_transfer.transfer(
                    &src_fs,
                    &request.src_path,
                    &dst_fs,
                    &request.dst_path,
                    request.mod_time,
                    &tmp_dir,
                    &bak_dest,
                    dry_run,
                )
            }
            _ => copyqueue_swaptransfer::TransferOutcome::Failed,
        };

        let (active_after, _) = semaphore.release();
        output.copy_slots(active_after, slot_limit);

        tries += 1;
        match outcome {
            copyqueue_swaptransfer::TransferOutcome::Done => {
                copy_succeeded = true;
                break;
            }
            copyqueue_swaptransfer::TransferOutcome::Failed if tries < try_limit => {
                continue;
            }
            _ => {
                output.diagnostic(&format!(
                    "copy failed: {} -> {}",
                    request.src_path, request.dst_path
                ));
                break;
            }
        }
    }
    if copy_succeeded {
        if let Some(cb) = on_success {
            cb();
        }
    }
    outstanding.dec();
}

struct CopyQueueImpl {
    transport: Arc<dyn transport::Transport>,
    output: Arc<dyn output::Output>,
    staging_cleanup: Arc<dyn copyqueue_stagingcleanup::StagingCleanup>,
    swap_transfer: Arc<dyn copyqueue_swaptransfer::SwapTransfer>,
    config: Mutex<ConfigState>,
    peer_cache: Arc<Mutex<HashMap<String, u64>>>,
    semaphore: Arc<Semaphore>,
    outstanding: Arc<Outstanding>,
    copy_id: AtomicU64,
}

impl CopyQueue for CopyQueueImpl {
    fn configure(&self, config: CopyConfig) {
        let slot_limit = config.copy_slot_limit.unwrap_or(DEFAULT_SLOT_LIMIT);
        let try_limit = config.copy_try_limit.unwrap_or(DEFAULT_TRY_LIMIT);
        self.semaphore.set_limit(slot_limit);
        let mut state = self.config.lock().unwrap();
        *state = ConfigState {
            slot_limit,
            try_limit,
            bak_retention: config
                .bak_retention
                .unwrap_or_else(|| Duration::from_secs(DEFAULT_BAK_SECS)),
            tmp_retention: config
                .tmp_retention
                .unwrap_or_else(|| Duration::from_secs(DEFAULT_TMP_SECS)),
            dry_run: config.dry_run,
        };
    }

    fn enqueue(&self, request: CopyRequest) {
        let (slot_limit, try_limit, dry_run) = {
            let cfg = self.config.lock().unwrap();
            (cfg.slot_limit, cfg.try_limit, cfg.dry_run)
        };
        let copy_id = self.copy_id.fetch_add(1, Ordering::Relaxed);
        let transport = Arc::clone(&self.transport);
        let output = Arc::clone(&self.output);
        let swap_transfer = Arc::clone(&self.swap_transfer);
        let peer_cache = Arc::clone(&self.peer_cache);
        let semaphore = Arc::clone(&self.semaphore);
        let outstanding = Arc::clone(&self.outstanding);
        outstanding.inc();
        thread::spawn(move || {
            run_copy(
                request,
                copy_id,
                try_limit,
                slot_limit,
                dry_run,
                transport,
                output,
                swap_transfer,
                peer_cache,
                semaphore,
                outstanding,
            );
        });
    }

    fn wait(&self) {
        self.outstanding.wait_zero();
    }

    fn run_in_parallel<'scope>(&self, jobs: Vec<Box<dyn FnOnce() + Send + 'scope>>) {
        std::thread::scope(|s| {
            for job in jobs {
                s.spawn(move || job());
            }
        });
    }

    fn recover_swap(&self, peer: &str, dir_path: &str) -> bool {
        let dry_run = self.config.lock().unwrap().dry_run;
        if dry_run {
            return true;
        }
        let peer_id = match open_peer_cached(&self.transport, &self.peer_cache, peer, false) {
            Some(id) => id,
            None => {
                self.output
                    .diagnostic(&format!("recover_swap: cannot connect to peer {}", peer));
                return false;
            }
        };
        let swap_dir = join_path(dir_path, ".kitchensync/SWAP");
        let entries = match self
            .transport
            .list_dir(&transport::PeerHandle(peer_id), &swap_dir)
        {
            Ok(e) => e,
            Err(transport::TransportError::NotFound) => return true,
            Err(_) => return false,
        };
        let fs = TransportFsAdapter {
            transport: Arc::clone(&self.transport),
            peer_id,
        };
        let ts = now_ts();
        let mut all_ok = true;
        for entry in &entries {
            if !entry.is_dir {
                continue;
            }
            let basename = percent_decode(&entry.name);
            let target_path = join_path(dir_path, &basename);
            let bak_dest =
                join_path(dir_path, &format!(".kitchensync/BAK/{}/{}", ts, &basename));
            if !self.swap_transfer.recover(&fs, &target_path, &bak_dest, false) {
                all_ok = false;
            }
        }
        all_ok
    }

    fn cleanup(&self, peer: &str, dir_path: &str) {
        let (dry_run, bak_days, tmp_days) = {
            let state = self.config.lock().unwrap();
            (
                state.dry_run,
                state.bak_retention.as_secs() / 86400,
                state.tmp_retention.as_secs() / 86400,
            )
        };
        if dry_run {
            return;
        }
        let peer_id = match open_peer_cached(&self.transport, &self.peer_cache, peer, false) {
            Some(id) => id,
            None => {
                self.output
                    .diagnostic(&format!("cleanup: cannot connect to peer {}", peer));
                return;
            }
        };
        let peer_fs = PeerFsAdapter {
            transport: Arc::clone(&self.transport),
            peer_id,
        };
        self.staging_cleanup.cleanup(
            &peer_fs,
            dir_path,
            Some(bak_days),
            Some(tmp_days),
            SystemTime::now(),
            false,
        );
    }
}

pub fn new(
    transport: Arc<dyn transport::Transport>,
    output: Arc<dyn output::Output>,
) -> Arc<dyn CopyQueue> {
    Arc::new(CopyQueueImpl {
        transport,
        output,
        staging_cleanup: copyqueue_stagingcleanup::new(),
        swap_transfer: copyqueue_swaptransfer::new(),
        config: Mutex::new(ConfigState::default()),
        peer_cache: Arc::new(Mutex::new(HashMap::new())),
        semaphore: Arc::new(Semaphore::new(DEFAULT_SLOT_LIMIT)),
        outstanding: Arc::new(Outstanding::new()),
        copy_id: AtomicU64::new(0),
    })
}
