use std::collections::HashMap;
use std::fs;
use std::io::{Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use snapshot::Snapshot;

// ---- infrastructure ----

struct FsState {
    peers: HashMap<u64, String>,
    reads: HashMap<u64, std::fs::File>,
    writes: HashMap<u64, std::fs::File>,
}

struct FileTransport {
    state: Mutex<FsState>,
    next_id: AtomicU64,
}

fn io_to_te(e: std::io::Error) -> transport::TransportError {
    match e.kind() {
        std::io::ErrorKind::NotFound => transport::TransportError::NotFound,
        std::io::ErrorKind::PermissionDenied => transport::TransportError::PermissionDenied,
        _ => transport::TransportError::Io,
    }
}

impl transport::Transport for FileTransport {
    fn normalize_url(&self, url: &str) -> String {
        url.to_string()
    }

    fn open_peer(
        &self,
        primary: &str,
        _fallbacks: &[String],
        _dry_run: bool,
        _timeout_conn: std::time::Duration,
    ) -> Option<transport::ConnectedPeer> {
        let root = primary.trim_start_matches("file://").to_string();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().peers.insert(id, root);
        Some(transport::ConnectedPeer {
            handle: transport::PeerHandle(id),
            winning_url: primary.to_string(),
        })
    }

    fn list_dir(
        &self,
        _peer: &transport::PeerHandle,
        _path: &str,
    ) -> Result<Vec<transport::DirEntry>, transport::TransportError> {
        Err(transport::TransportError::Io)
    }

    fn stat(
        &self,
        peer: &transport::PeerHandle,
        path: &str,
    ) -> Result<transport::Stat, transport::TransportError> {
        let root = self.state.lock().unwrap().peers[&peer.0].clone();
        let full = PathBuf::from(root).join(path);
        let meta = std::fs::metadata(&full).map_err(io_to_te)?;
        Ok(transport::Stat {
            mod_time: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            byte_size: if meta.is_dir() { -1 } else { meta.len() as i64 },
            is_dir: meta.is_dir(),
        })
    }

    fn open_read(
        &self,
        peer: &transport::PeerHandle,
        path: &str,
    ) -> Result<transport::ReadHandle, transport::TransportError> {
        let root = self.state.lock().unwrap().peers[&peer.0].clone();
        let full = PathBuf::from(root).join(path);
        let f = std::fs::File::open(&full).map_err(io_to_te)?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().reads.insert(id, f);
        Ok(transport::ReadHandle(id))
    }

    fn read(
        &self,
        handle: &transport::ReadHandle,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, transport::TransportError> {
        let mut state = self.state.lock().unwrap();
        let f = state.reads.get_mut(&handle.0).ok_or(transport::TransportError::Io)?;
        let mut buf = vec![0u8; max_bytes];
        let n = f.read(&mut buf).map_err(io_to_te)?;
        if n == 0 {
            Ok(None)
        } else {
            buf.truncate(n);
            Ok(Some(buf))
        }
    }

    fn close_read(
        &self,
        handle: transport::ReadHandle,
    ) -> Result<(), transport::TransportError> {
        self.state.lock().unwrap().reads.remove(&handle.0);
        Ok(())
    }

    fn open_write(
        &self,
        peer: &transport::PeerHandle,
        path: &str,
    ) -> Result<transport::WriteHandle, transport::TransportError> {
        let root = self.state.lock().unwrap().peers[&peer.0].clone();
        let full = PathBuf::from(root).join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(io_to_te)?;
        }
        let f = std::fs::File::create(&full).map_err(io_to_te)?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().writes.insert(id, f);
        Ok(transport::WriteHandle(id))
    }

    fn write(
        &self,
        handle: &transport::WriteHandle,
        bytes: &[u8],
    ) -> Result<(), transport::TransportError> {
        let mut state = self.state.lock().unwrap();
        let f = state.writes.get_mut(&handle.0).ok_or(transport::TransportError::Io)?;
        f.write_all(bytes).map_err(io_to_te)
    }

    fn close_write(
        &self,
        handle: transport::WriteHandle,
    ) -> Result<(), transport::TransportError> {
        self.state.lock().unwrap().writes.remove(&handle.0);
        Ok(())
    }

    fn create_dir(
        &self,
        _peer: &transport::PeerHandle,
        _path: &str,
    ) -> Result<(), transport::TransportError> {
        Err(transport::TransportError::Io)
    }

    fn rename(
        &self,
        peer: &transport::PeerHandle,
        src: &str,
        dst: &str,
    ) -> Result<(), transport::TransportError> {
        let root = self.state.lock().unwrap().peers[&peer.0].clone();
        let from = PathBuf::from(&root).join(src);
        let to = PathBuf::from(&root).join(dst);
        if to.exists() {
            return Err(transport::TransportError::Io);
        }
        std::fs::rename(&from, &to).map_err(io_to_te)
    }

    fn delete_file(
        &self,
        peer: &transport::PeerHandle,
        path: &str,
    ) -> Result<(), transport::TransportError> {
        let root = self.state.lock().unwrap().peers[&peer.0].clone();
        let full = PathBuf::from(root).join(path);
        std::fs::remove_file(&full).map_err(io_to_te)
    }

    fn delete_dir(
        &self,
        _peer: &transport::PeerHandle,
        _path: &str,
    ) -> Result<(), transport::TransportError> {
        Err(transport::TransportError::Io)
    }

    fn set_mod_time(
        &self,
        _peer: &transport::PeerHandle,
        _path: &str,
        _time: std::time::SystemTime,
    ) -> Result<(), transport::TransportError> {
        Ok(())
    }
}

fn make_transport() -> Arc<dyn transport::Transport> {
    Arc::new(FileTransport {
        state: Mutex::new(FsState {
            peers: HashMap::new(),
            reads: HashMap::new(),
            writes: HashMap::new(),
        }),
        next_id: AtomicU64::new(1),
    })
}

fn test_peer(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("snap_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn peer_url(dir: &Path) -> String {
    format!("file://{}", dir.display())
}

fn make() -> Arc<dyn Snapshot> {
    snapshot::new(make_transport())
}

fn db_path(peer_dir: &Path) -> PathBuf {
    peer_dir.join(".kitchensync/snapshot.db")
}

fn sqlite3(db: &Path, sql: &str) -> String {
    let out = Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .output()
        .expect("sqlite3 must be on PATH for schema inspection");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn create_snapshot_db(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let status = Command::new("sqlite3")
        .arg(path)
        .arg(concat!(
            "CREATE TABLE IF NOT EXISTS snapshot (",
            "id TEXT PRIMARY KEY, parent_id TEXT, basename TEXT NOT NULL, ",
            "mod_time TEXT NOT NULL, byte_size INTEGER NOT NULL, ",
            "last_seen TEXT, deleted_time TEXT);",
            "CREATE INDEX IF NOT EXISTS idx_parent_id ON snapshot(parent_id);",
            "CREATE INDEX IF NOT EXISTS idx_last_seen ON snapshot(last_seen);",
            "CREATE INDEX IF NOT EXISTS idx_deleted_time ON snapshot(deleted_time);",
        ))
        .status()
        .expect("sqlite3 must be on PATH");
    assert!(status.success(), "failed to create snapshot DB at {:?}", path);
}

fn insert_row(
    db: &Path,
    id: &str,
    parent_id: &str,
    basename: &str,
    mod_time: &str,
    byte_size: i64,
    last_seen: Option<&str>,
    deleted_time: Option<&str>,
) {
    let ls = last_seen
        .map(|s| format!("'{}'", s))
        .unwrap_or_else(|| "NULL".to_string());
    let dt = deleted_time
        .map(|s| format!("'{}'", s))
        .unwrap_or_else(|| "NULL".to_string());
    let sql = format!(
        "INSERT OR REPLACE INTO snapshot \
         (id,parent_id,basename,mod_time,byte_size,last_seen,deleted_time) \
         VALUES ('{}','{}','{}','{}',{},{},{});",
        id, parent_id, basename, mod_time, byte_size, ls, dt
    );
    let status = Command::new("sqlite3")
        .arg(db)
        .arg(&sql)
        .status()
        .expect("sqlite3 must be on PATH");
    assert!(status.success(), "failed to insert row into {:?}", db);
}

fn is_base62(c: char) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase() || c.is_ascii_lowercase()
}

// YYYY-MM-DD_HH-mm-ss_ffffffZ  (27 characters)
fn is_timestamp(ts: &str) -> bool {
    if ts.len() != 27 {
        return false;
    }
    let b = ts.as_bytes();
    b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'_'
        && b[13] == b'-'
        && b[16] == b'-'
        && b[19] == b'_'
        && b[26] == b'Z'
        && ts[..4].bytes().all(|c| c.is_ascii_digit())
        && ts[5..7].bytes().all(|c| c.is_ascii_digit())
        && ts[8..10].bytes().all(|c| c.is_ascii_digit())
        && ts[11..13].bytes().all(|c| c.is_ascii_digit())
        && ts[14..16].bytes().all(|c| c.is_ascii_digit())
        && ts[17..19].bytes().all(|c| c.is_ascii_digit())
        && ts[20..26].bytes().all(|c| c.is_ascii_digit())
}

fn open_peer(name: &str) -> (PathBuf, Arc<dyn Snapshot>, String) {
    let dir = test_peer(name);
    let s = make();
    let peer = peer_url(&dir);
    s.open(&peer, false).unwrap();
    (dir, s, peer)
}

// ---- 014: path_identity format and properties ----

#[test]
fn path_identity_is_eleven_chars() {
    // 014.3: zero-padded to an 11-character string.
    let s = make();
    assert_eq!(s.path_identity("docs/readme.txt").len(), 11);
    assert_eq!(s.path_identity("docs/notes").len(), 11);
    assert_eq!(s.path_identity("file.txt").len(), 11);
    assert_eq!(s.path_identity("/").len(), 11);
}

#[test]
fn path_identity_uses_base62_charset() {
    // 014.2: digits 0-9, then A-Z, then a-z.
    let s = make();
    for path in &["docs/readme.txt", "docs/notes", "a", "/"] {
        let id = s.path_identity(path);
        for ch in id.chars() {
            assert!(is_base62(ch), "non-base62 char {:?} in identity for {:?}", ch, path);
        }
    }
}

#[test]
fn path_identity_is_deterministic() {
    // 014.1: the same path always produces the same identity.
    let s = make();
    let a = s.path_identity("docs/readme.txt");
    let b = s.path_identity("docs/readme.txt");
    assert_eq!(a, b);
}

#[test]
fn path_identity_file_and_dir_same_path_equal() {
    // 014.7: a file and a directory with the same canonical path share an identity.
    // Type is not part of the input; two calls with the same path string must agree.
    let s = make();
    let as_file = s.path_identity("docs/notes");
    let as_dir = s.path_identity("docs/notes");
    assert_eq!(as_file, as_dir);
}

#[test]
fn path_identity_docs_readme_txt_example() {
    // 014.8: the identity of docs/readme.txt is the xxHash64(0) of "docs/readme.txt".
    let s = make();
    let id = s.path_identity("docs/readme.txt");
    assert_eq!(id.len(), 11);
    assert!(id.chars().all(is_base62));
    assert_eq!(id, s.path_identity("docs/readme.txt"));
}

#[test]
fn path_identity_docs_notes_dir_example() {
    // 014.9: the identity of directory docs/notes is the hash of "docs/notes".
    let s = make();
    let id = s.path_identity("docs/notes");
    assert_eq!(id.len(), 11);
    assert!(id.chars().all(is_base62));
    assert_eq!(id, s.path_identity("docs/notes"));
}

#[test]
fn path_identity_parent_of_docs_entries_is_hash_of_docs() {
    // 014.10, 014.11: parent identity of docs/readme.txt and docs/notes is hash of "docs".
    let s = make();
    let parent = s.path_identity("docs");
    assert_eq!(parent, s.path_identity("docs"));
    assert_ne!(parent, s.path_identity("docs/readme.txt"));
    assert_ne!(parent, s.path_identity("docs/notes"));
}

#[test]
fn path_identity_root_level_parent_is_sentinel() {
    // 014.12: parent identity of a root-level entry is the hash of the sentinel "/".
    let s = make();
    let sentinel = s.path_identity("/");
    assert_eq!(sentinel.len(), 11);
    assert!(sentinel.chars().all(is_base62));
    assert_ne!(sentinel, s.path_identity("file.txt"));
}

#[test]
fn path_identity_distinct_paths_produce_distinct_ids() {
    // 014.1, 014.4: different canonical paths hash to different identities.
    let s = make();
    let ids = [
        s.path_identity("docs/readme.txt"),
        s.path_identity("docs/notes"),
        s.path_identity("docs"),
        s.path_identity("readme.txt"),
        s.path_identity("/"),
    ];
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j], "paths at index {} and {} must differ", i, j);
        }
    }
}

#[test]
fn path_identity_forward_slash_canonical() {
    // 014.4: canonical path uses forward slashes.
    let s = make();
    let fwd = s.path_identity("a/b/c");
    assert_eq!(fwd, s.path_identity("a/b/c"));
}

#[test]
fn path_identity_no_leading_slash() {
    // 014.5: canonical path has no leading slash; "/file.txt" normalizes to "file.txt".
    let s = make();
    assert_eq!(s.path_identity("file.txt"), s.path_identity("/file.txt"));
}

#[test]
fn path_identity_no_trailing_slash() {
    // 014.6: canonical path has no trailing slash; "docs/notes/" normalizes to "docs/notes".
    let s = make();
    assert_eq!(s.path_identity("docs/notes"), s.path_identity("docs/notes/"));
}

// ---- 015: timestamps ----

#[test]
fn now_matches_timestamp_format() {
    // 015.1, 015.2, 015.3: YYYY-MM-DD_HH-mm-ss_ffffffZ, UTC, microsecond precision.
    let s = make();
    let ts = s.now();
    assert!(
        is_timestamp(&ts),
        "now() returned {:?} which does not match YYYY-MM-DD_HH-mm-ss_ffffffZ",
        ts
    );
    assert!(ts.ends_with('Z'), "timestamp must be UTC (end with Z)");
}

#[test]
fn now_is_strictly_increasing() {
    // 015.8: no two freshly generated timestamps in one run are equal.
    let s = make();
    let mut prev = s.now();
    for _ in 0..20 {
        let next = s.now();
        assert!(
            next > prev,
            "now() must return strictly increasing values; got {:?} then {:?}",
            prev,
            next
        );
        prev = next;
    }
}

#[test]
fn now_lexicographic_order_matches_chronological() {
    // 015.4: sorting timestamp values as plain strings orders them chronologically.
    let s = make();
    let t1 = s.now();
    let t2 = s.now();
    assert!(t1 < t2, "lexicographic comparison must equal chronological order");
}

// ---- 013, 016: schema after open + writeback ----

fn fresh_db(name: &str) -> PathBuf {
    let (dir, s, peer) = open_peer(name);
    s.writeback(&peer, false).unwrap();
    dir
}

#[test]
fn schema_exactly_one_table() {
    // 013.1: exactly one table.
    let dir = fresh_db("schema_one_tbl");
    let out = sqlite3(
        &db_path(&dir),
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;",
    );
    assert_eq!(out.lines().count(), 1, "expected 1 table, got: {:?}", out);
}

#[test]
fn schema_table_named_snapshot() {
    // 013.2: the single table is named `snapshot`.
    let dir = fresh_db("schema_tbl_name");
    let out = sqlite3(
        &db_path(&dir),
        "SELECT name FROM sqlite_master WHERE type='table';",
    );
    assert_eq!(out.trim(), "snapshot");
}

#[test]
fn schema_no_view() {
    // 013.3: no view.
    let dir = fresh_db("schema_no_view");
    let out = sqlite3(
        &db_path(&dir),
        "SELECT name FROM sqlite_master WHERE type='view';",
    );
    assert!(out.is_empty(), "expected no views, got: {:?}", out);
}

#[test]
fn schema_columns_types_and_constraints() {
    // 013.4-013.16: all columns present with correct types and NOT NULL / nullable constraints.
    let dir = fresh_db("schema_cols");
    let db = db_path(&dir);
    // PRAGMA table_info rows: cid|name|type|notnull|dflt_value|pk
    let info = sqlite3(&db, "PRAGMA table_info(snapshot);");

    let col = |name: &str| -> Vec<String> {
        info.lines()
            .find(|l| {
                let parts: Vec<&str> = l.split('|').collect();
                parts.get(1).copied() == Some(name)
            })
            .map(|l| l.split('|').map(str::to_string).collect())
            .unwrap_or_default()
    };

    // id: TEXT, primary key (pk != 0)
    let id = col("id");
    assert!(!id.is_empty(), "column id must exist");
    assert_eq!(id.get(2).map(String::as_str), Some("TEXT"), "id must be TEXT"); // 013.4
    assert!(id.get(5).map(|s| s != "0").unwrap_or(false), "id must be primary key"); // 013.5

    // parent_id: TEXT, nullable
    let pid = col("parent_id");
    assert!(!pid.is_empty(), "column parent_id must exist");
    assert_eq!(pid.get(2).map(String::as_str), Some("TEXT"), "parent_id must be TEXT"); // 013.6

    // basename: TEXT, NOT NULL
    let bn = col("basename");
    assert!(!bn.is_empty(), "column basename must exist");
    assert_eq!(bn.get(2).map(String::as_str), Some("TEXT"), "basename must be TEXT"); // 013.7
    assert_eq!(bn.get(3).map(String::as_str), Some("1"), "basename must be NOT NULL"); // 013.8

    // mod_time: TEXT, NOT NULL
    let mt = col("mod_time");
    assert!(!mt.is_empty(), "column mod_time must exist");
    assert_eq!(mt.get(2).map(String::as_str), Some("TEXT"), "mod_time must be TEXT"); // 013.9
    assert_eq!(mt.get(3).map(String::as_str), Some("1"), "mod_time must be NOT NULL"); // 013.10

    // byte_size: INTEGER, NOT NULL
    let bs = col("byte_size");
    assert!(!bs.is_empty(), "column byte_size must exist");
    assert_eq!(bs.get(2).map(String::as_str), Some("INTEGER"), "byte_size must be INTEGER"); // 013.11
    assert_eq!(bs.get(3).map(String::as_str), Some("1"), "byte_size must be NOT NULL"); // 013.12

    // last_seen: TEXT, nullable
    let ls = col("last_seen");
    assert!(!ls.is_empty(), "column last_seen must exist");
    assert_eq!(ls.get(2).map(String::as_str), Some("TEXT"), "last_seen must be TEXT"); // 013.15
    assert_eq!(ls.get(3).map(String::as_str), Some("0"), "last_seen must permit NULL"); // 013.15

    // deleted_time: TEXT, nullable
    let dt = col("deleted_time");
    assert!(!dt.is_empty(), "column deleted_time must exist");
    assert_eq!(dt.get(2).map(String::as_str), Some("TEXT"), "deleted_time must be TEXT"); // 013.16
    assert_eq!(dt.get(3).map(String::as_str), Some("0"), "deleted_time must permit NULL"); // 013.16
}

fn has_index_on(db: &Path, column: &str) -> bool {
    let idx_list = sqlite3(db, "PRAGMA index_list(snapshot);");
    for line in idx_list.lines() {
        let name = match line.split('|').nth(1) {
            Some(n) => n,
            None => continue,
        };
        let idx_info = sqlite3(db, &format!("PRAGMA index_info({});", name));
        if idx_info.lines().any(|l| l.split('|').nth(2) == Some(column)) {
            return true;
        }
    }
    false
}

#[test]
fn schema_index_on_parent_id() {
    // 013.17
    let dir = fresh_db("schema_idx_pid");
    assert!(has_index_on(&db_path(&dir), "parent_id"), "must have index on parent_id");
}

#[test]
fn schema_index_on_last_seen() {
    // 013.18
    let dir = fresh_db("schema_idx_ls");
    assert!(has_index_on(&db_path(&dir), "last_seen"), "must have index on last_seen");
}

#[test]
fn schema_index_on_deleted_time() {
    // 013.19
    let dir = fresh_db("schema_idx_dt");
    assert!(has_index_on(&db_path(&dir), "deleted_time"), "must have index on deleted_time");
}

// ---- 016.2: rollback-journal mode ----

#[test]
fn writeback_db_is_rollback_journal_mode() {
    // 016.2: snapshot.db is a SQLite database in rollback-journal mode (not WAL).
    let dir = fresh_db("rj_mode");
    let db = db_path(&dir);
    let mode = sqlite3(&db, "PRAGMA journal_mode;");
    assert_eq!(mode.trim(), "delete", "snapshot.db must use rollback-journal (delete) mode");
}

// ---- 016.3: no sidecar files at the peer ----

#[test]
fn writeback_leaves_no_sidecar_files() {
    // 016.3: SQLite sidecar files are never uploaded; no .db-wal or .db-shm at peer.
    let dir = fresh_db("no_sidecars");
    let db = db_path(&dir);
    assert!(!db.with_extension("db-wal").exists(), "snapshot.db-wal must not exist after writeback");
    assert!(!db.with_extension("db-shm").exists(), "snapshot.db-shm must not exist after writeback");
}

// ---- 016.6: fresh peer with no snapshot creates empty DB ----

#[test]
fn open_fresh_peer_creates_empty_database() {
    // 016.6: no existing snapshot.db → new empty database created locally.
    let (_, s, peer) = open_peer("fresh_db");
    let result = s.read_row(&peer, "00000000000").unwrap();
    assert!(result.is_none(), "fresh database must have no rows");
}

// ---- 016.1, 016.7: writeback places snapshot at .kitchensync/snapshot.db ----

#[test]
fn writeback_places_db_at_kitchensync_path() {
    // 016.1: {peer-root}/.kitchensync/snapshot.db
    let (dir, s, peer) = open_peer("wb_path");
    s.writeback(&peer, false).unwrap();
    assert!(db_path(&dir).exists(), "snapshot.db must exist at .kitchensync/snapshot.db");
}

#[test]
fn writeback_leaves_no_swap_remnants() {
    // 016.8-016.12: after successful writeback SWAP is clean.
    let (dir, s, peer) = open_peer("wb_no_swap");
    s.writeback(&peer, false).unwrap();
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    if swap.exists() {
        let leftover: Vec<_> = fs::read_dir(&swap).unwrap().collect();
        assert!(leftover.is_empty(), "SWAP/snapshot.db must be empty after writeback");
    }
}

#[test]
fn writeback_data_survives_roundtrip() {
    // 016.7: the uploaded file opens standalone with all changes committed.
    let (dir, s, peer) = open_peer("wb_rt");
    let id = s.path_identity("rt.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "rt.txt", &mt, 1234).unwrap();
    s.writeback(&peer, false).unwrap();

    let s2 = make();
    let peer2 = peer_url(&dir);
    s2.open(&peer2, false).unwrap();
    let row = s2.read_row(&peer2, &id).unwrap().expect("row must survive writeback roundtrip");
    assert_eq!(row.byte_size, 1234);
    assert_eq!(row.mod_time, mt);
}

// ---- 013.13, 013.14: byte_size for file vs directory ----

#[test]
fn record_present_file_stores_byte_size() {
    // 013.13: file row records actual size in bytes.
    let (_, s, peer) = open_peer("rp_file_bs");
    let id = s.path_identity("docs/readme.txt");
    let pid = s.path_identity("docs");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "readme.txt", &mt, 4096).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert_eq!(row.byte_size, 4096);
}

#[test]
fn record_present_directory_stores_minus_one() {
    // 013.14: directory row stores -1 in byte_size.
    let (_, s, peer) = open_peer("rp_dir_bs");
    let id = s.path_identity("docs/notes");
    let pid = s.path_identity("docs");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "notes", &mt, -1).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert_eq!(row.byte_size, -1);
}

// ---- 013.20: at most one row per tracked path ----

#[test]
fn record_present_upserts_existing_row() {
    // 013.20: at most one row per tracked path.
    let (_, s, peer) = open_peer("rp_upsert");
    let id = s.path_identity("docs/readme.txt");
    let pid = s.path_identity("docs");
    let mt1 = s.now();
    s.record_present(&peer, &id, &pid, "readme.txt", &mt1, 100).unwrap();
    let mt2 = s.now();
    s.record_present(&peer, &id, &pid, "readme.txt", &mt2, 200).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert_eq!(row.byte_size, 200, "second call must overwrite, not duplicate");
    assert_eq!(row.mod_time, mt2);
}

// ---- 017.1-017.4: record_present ----

#[test]
fn record_present_stores_all_fields() {
    // 017.1, 017.2: mod_time and byte_size recorded; all row fields match.
    let (_, s, peer) = open_peer("rp_fields");
    let id = s.path_identity("data/file.bin");
    let pid = s.path_identity("data");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "file.bin", &mt, 8192).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert_eq!(row.id, id);
    assert_eq!(row.parent_id, pid);
    assert_eq!(row.basename, "file.bin");
    assert_eq!(row.mod_time, mt);
    assert_eq!(row.byte_size, 8192);
}

#[test]
fn record_present_sets_last_seen_fresh_timestamp() {
    // 017.3, 015.6: last_seen is set to a fresh timestamp from now().
    let (_, s, peer) = open_peer("rp_ls");
    let id = s.path_identity("file.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "file.txt", &mt, 100).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    let last_seen = row.last_seen.expect("last_seen must be set");
    assert!(is_timestamp(&last_seen), "last_seen must be a valid timestamp");
}

#[test]
fn record_present_clears_deleted_time_to_null() {
    // 017.4: deleted_time is cleared to NULL on confirmed-present.
    let (_, s, peer) = open_peer("rp_clear_dt");
    let id = s.path_identity("file.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "file.txt", &mt, 10).unwrap();
    s.record_absent(&peer, &id).unwrap();
    assert!(s.read_row(&peer, &id).unwrap().unwrap().deleted_time.is_some());

    let mt2 = s.now();
    s.record_present(&peer, &id, &pid, "file.txt", &mt2, 10).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert!(row.deleted_time.is_none(), "record_present must clear deleted_time");
}

// ---- 017.5-017.7, 015.9: record_absent ----

#[test]
fn record_absent_on_live_row_sets_deleted_time_from_last_seen() {
    // 017.5, 015.9: deleted_time is set from the row's existing last_seen (not a fresh timestamp).
    let (_, s, peer) = open_peer("ra_live");
    let id = s.path_identity("gone.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "gone.txt", &mt, 50).unwrap();
    let last_seen = s.read_row(&peer, &id).unwrap().unwrap().last_seen.expect("last_seen must be set");
    s.record_absent(&peer, &id).unwrap();
    let after = s.read_row(&peer, &id).unwrap().expect("row must still exist");
    assert_eq!(
        after.deleted_time.as_deref(),
        Some(last_seen.as_str()),
        "deleted_time must be copied from the row's last_seen"
    );
}

#[test]
fn record_absent_leaves_last_seen_unchanged() {
    // 017.6: last_seen is not touched by record_absent.
    let (_, s, peer) = open_peer("ra_ls_unchanged");
    let id = s.path_identity("orphan.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "orphan.txt", &mt, 10).unwrap();
    let ls_before = s.read_row(&peer, &id).unwrap().unwrap().last_seen.expect("last_seen set");
    s.record_absent(&peer, &id).unwrap();
    let after = s.read_row(&peer, &id).unwrap().unwrap();
    assert_eq!(after.last_seen.as_deref(), Some(ls_before.as_str()), "last_seen must not change");
}

#[test]
fn record_absent_on_tombstoned_row_is_noop() {
    // 017.7: a row with deleted_time already set is left unchanged.
    let (_, s, peer) = open_peer("ra_noop");
    let id = s.path_identity("already.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "already.txt", &mt, 10).unwrap();
    s.record_absent(&peer, &id).unwrap();
    let dt_first = s.read_row(&peer, &id).unwrap().unwrap().deleted_time.clone();
    s.record_absent(&peer, &id).unwrap();
    let dt_second = s.read_row(&peer, &id).unwrap().unwrap().deleted_time;
    assert_eq!(dt_second, dt_first, "second record_absent must leave deleted_time unchanged");
}

// ---- 017.8-017.11, 017.21, 017.22: record_push ----

#[test]
fn record_push_creates_row_with_null_last_seen() {
    // 017.11, 017.21, 017.22: last_seen stays NULL when no prior row exists.
    let (_, s, peer) = open_peer("push_null_ls");
    let id = s.path_identity("incoming.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_push(&peer, &id, &pid, "incoming.txt", &mt, 512).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert!(row.last_seen.is_none(), "last_seen must be NULL after record_push with no prior row");
}

#[test]
fn record_push_sets_null_deleted_time() {
    // 017.10: deleted_time is NULL on a push-decision row.
    let (_, s, peer) = open_peer("push_null_dt");
    let id = s.path_identity("pushed.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_push(&peer, &id, &pid, "pushed.txt", &mt, 1024).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert!(row.deleted_time.is_none(), "deleted_time must be NULL after record_push");
}

#[test]
fn record_push_stores_mod_time_and_byte_size() {
    // 017.8, 017.9: winning mod_time and byte_size recorded.
    let (_, s, peer) = open_peer("push_fields");
    let id = s.path_identity("pushed2.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_push(&peer, &id, &pid, "pushed2.txt", &mt, 2048).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert_eq!(row.mod_time, mt);
    assert_eq!(row.byte_size, 2048);
}

// ---- 017.12, 017.13: record_copied ----

#[test]
fn record_copied_sets_last_seen_to_fresh_timestamp() {
    // 017.12: after copy completes, last_seen gets a fresh timestamp.
    let (_, s, peer) = open_peer("copied_ls");
    let id = s.path_identity("copied.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_push(&peer, &id, &pid, "copied.txt", &mt, 256).unwrap();
    assert!(s.read_row(&peer, &id).unwrap().unwrap().last_seen.is_none());
    s.record_copied(&peer, &id).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must still exist");
    let last_seen = row.last_seen.expect("last_seen must be set after record_copied");
    assert!(is_timestamp(&last_seen), "last_seen must be a valid timestamp");
}

// ---- 017.14: failed inline op leaves row unchanged ----

#[test]
fn failed_inline_op_leaves_row_unchanged() {
    // 017.14: the caller does not call record_copied on failure, so the row stays as-is.
    let (_, s, peer) = open_peer("fail_inline");
    let id = s.path_identity("target.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_push(&peer, &id, &pid, "target.txt", &mt, 100).unwrap();
    // Simulate failure: do NOT call record_copied.
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert!(row.last_seen.is_none(), "row must be unchanged when copy did not complete");
    assert!(row.deleted_time.is_none(), "deleted_time must remain NULL");
}

// ---- 017.15-017.18, 015.10: record_displaced + cascade ----

#[test]
fn record_displaced_sets_deleted_time_to_last_seen() {
    // 017.15: deleted_time set to the row's current last_seen.
    let (_, s, peer) = open_peer("disp_dt");
    let id = s.path_identity("old_file.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "old_file.txt", &mt, 100).unwrap();
    let last_seen = s.read_row(&peer, &id).unwrap().unwrap().last_seen.expect("last_seen set");
    s.record_displaced(&peer, &id).unwrap();
    let after = s.read_row(&peer, &id).unwrap().expect("row must still exist");
    assert_eq!(
        after.deleted_time.as_deref(),
        Some(last_seen.as_str()),
        "deleted_time must be set to last_seen on displacement"
    );
}

#[test]
fn record_displaced_cascades_deleted_time_to_descendants() {
    // 017.16, 015.10: descendant rows receive the displaced entry's deleted_time value.
    let (_, s, peer) = open_peer("disp_cascade");
    let par_id = s.path_identity("subtree");
    let par_pid = s.path_identity("/");
    let c1_id = s.path_identity("subtree/child1.txt");
    let c2_id = s.path_identity("subtree/child2.txt");

    let t = s.now();
    s.record_present(&peer, &par_id, &par_pid, "subtree", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer, &c1_id, &par_id, "child1.txt", &t, 100).unwrap();
    let t = s.now();
    s.record_present(&peer, &c2_id, &par_id, "child2.txt", &t, 200).unwrap();

    s.record_displaced(&peer, &par_id).unwrap();

    let par = s.read_row(&peer, &par_id).unwrap().expect("parent row must exist");
    let c1 = s.read_row(&peer, &c1_id).unwrap().expect("child1 row must exist");
    let c2 = s.read_row(&peer, &c2_id).unwrap().expect("child2 row must exist");

    assert!(c1.deleted_time.is_some(), "child1 must be tombstoned by cascade");
    assert!(c2.deleted_time.is_some(), "child2 must be tombstoned by cascade");
    assert_eq!(c1.deleted_time, par.deleted_time, "child1 deleted_time must equal parent's");
    assert_eq!(c2.deleted_time, par.deleted_time, "child2 deleted_time must equal parent's");
}

#[test]
fn record_displaced_cascade_skips_already_tombstoned_descendants() {
    // 017.18: cascade does not overwrite deleted_time on already-tombstoned rows.
    let (_, s, peer) = open_peer("disp_no_ow");
    let par_id = s.path_identity("dir");
    let par_pid = s.path_identity("/");
    let child_id = s.path_identity("dir/child.txt");

    let t = s.now();
    s.record_present(&peer, &par_id, &par_pid, "dir", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer, &child_id, &par_id, "child.txt", &t, 50).unwrap();
    s.record_absent(&peer, &child_id).unwrap();
    let dt_before = s
        .read_row(&peer, &child_id)
        .unwrap()
        .unwrap()
        .deleted_time
        .clone()
        .expect("child must be tombstoned");

    s.record_displaced(&peer, &par_id).unwrap();

    let child_after = s.read_row(&peer, &child_id).unwrap().expect("child must still exist");
    assert_eq!(
        child_after.deleted_time.as_deref(),
        Some(dt_before.as_str()),
        "cascade must not overwrite an already-set deleted_time"
    );
}

#[test]
fn record_displaced_only_touches_descendants_not_siblings() {
    // 017.17: cascade only touches rows reachable as descendants through parent_id.
    let (_, s, peer) = open_peer("disp_no_sib");
    let root = s.path_identity("/");
    let target_id = s.path_identity("to_displace");
    let child_id = s.path_identity("to_displace/child.txt");
    let sibling_id = s.path_identity("sibling.txt");

    let t = s.now();
    s.record_present(&peer, &target_id, &root, "to_displace", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer, &child_id, &target_id, "child.txt", &t, 10).unwrap();
    let t = s.now();
    s.record_present(&peer, &sibling_id, &root, "sibling.txt", &t, 20).unwrap();

    s.record_displaced(&peer, &target_id).unwrap();

    let sibling = s.read_row(&peer, &sibling_id).unwrap().expect("sibling must exist");
    assert!(
        sibling.deleted_time.is_none(),
        "sibling of displaced entry must not be tombstoned"
    );
}

// 017.16: cascade is transitive (grandchildren are also tombstoned).
#[test]
fn record_displaced_cascade_is_transitive() {
    let (_, s, peer) = open_peer("disp_transitive");
    let root_id = s.path_identity("/");
    let dir_id = s.path_identity("top");
    let sub_id = s.path_identity("top/sub");
    let leaf_id = s.path_identity("top/sub/leaf.txt");

    let t = s.now();
    s.record_present(&peer, &dir_id, &root_id, "top", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer, &sub_id, &dir_id, "sub", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer, &leaf_id, &sub_id, "leaf.txt", &t, 10).unwrap();

    s.record_displaced(&peer, &dir_id).unwrap();

    let leaf = s.read_row(&peer, &leaf_id).unwrap().expect("grandchild row must exist");
    assert!(
        leaf.deleted_time.is_some(),
        "grandchild must be tombstoned by transitive cascade (017.16)"
    );
}

// ---- 017.19, 017.20: displacement cascade is per-peer ----

#[test]
fn record_displaced_cascade_does_not_touch_other_peer() {
    // 017.19: the cascade runs against the displaced peer's own snapshot DB only and never
    // against another peer's snapshot database.
    let dir_a = test_peer("iso_cascade_a");
    let dir_b = test_peer("iso_cascade_b");
    let s = make();
    let peer_a = peer_url(&dir_a);
    let peer_b = peer_url(&dir_b);
    s.open(&peer_a, false).unwrap();
    s.open(&peer_b, false).unwrap();

    let dir_id = s.path_identity("shared");
    let dir_pid = s.path_identity("/");
    let child_id = s.path_identity("shared/file.txt");

    let t = s.now();
    s.record_present(&peer_a, &dir_id, &dir_pid, "shared", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer_a, &child_id, &dir_id, "file.txt", &t, 100).unwrap();
    let t = s.now();
    s.record_present(&peer_b, &dir_id, &dir_pid, "shared", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer_b, &child_id, &dir_id, "file.txt", &t, 100).unwrap();

    s.record_displaced(&peer_a, &dir_id).unwrap();

    let b_child = s.read_row(&peer_b, &child_id).unwrap().expect("peer B child must exist");
    assert!(
        b_child.deleted_time.is_none(),
        "cascade on peer A must not tombstone peer B's rows (017.19)"
    );
}

#[test]
fn record_displaced_cascade_runs_independently_per_peer() {
    // 017.20: when several peers lose the same subtree, the cascade runs once per peer after
    // that peer's displacement succeeds, each against that peer's own snapshot database.
    let dir_a = test_peer("cascade_per_a");
    let dir_b = test_peer("cascade_per_b");
    let s = make();
    let peer_a = peer_url(&dir_a);
    let peer_b = peer_url(&dir_b);
    s.open(&peer_a, false).unwrap();
    s.open(&peer_b, false).unwrap();

    let dir_id = s.path_identity("lost");
    let dir_pid = s.path_identity("/");
    let child_id = s.path_identity("lost/item.bin");

    let t = s.now();
    s.record_present(&peer_a, &dir_id, &dir_pid, "lost", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer_a, &child_id, &dir_id, "item.bin", &t, 77).unwrap();
    let t = s.now();
    s.record_present(&peer_b, &dir_id, &dir_pid, "lost", &t, -1).unwrap();
    let t = s.now();
    s.record_present(&peer_b, &child_id, &dir_id, "item.bin", &t, 77).unwrap();

    s.record_displaced(&peer_a, &dir_id).unwrap();
    assert!(
        s.read_row(&peer_a, &child_id).unwrap().unwrap().deleted_time.is_some(),
        "peer A child must be tombstoned after A's cascade"
    );
    assert!(
        s.read_row(&peer_b, &child_id).unwrap().unwrap().deleted_time.is_none(),
        "peer B child must be untouched before B's cascade runs (017.19)"
    );

    s.record_displaced(&peer_b, &dir_id).unwrap();
    assert!(
        s.read_row(&peer_b, &child_id).unwrap().unwrap().deleted_time.is_some(),
        "peer B child must be tombstoned after B's own cascade (017.20)"
    );
}

// ---- 018: prune ----
//
// Pre-populate snapshot.db on the peer using sqlite3, then open and call prune.

fn peer_with_preloaded_rows(name: &str) -> (PathBuf, Arc<dyn Snapshot>, String) {
    let dir = test_peer(name);
    let db = db_path(&dir);
    create_snapshot_db(&db);

    // Old tombstone: deleted 2016-01-01 (well beyond 30 days from any current run).
    insert_row(
        &db,
        "old00000001",
        "0sentinel00",
        "old_del.txt",
        "2016-01-01_00-00-00_000000Z",
        100,
        Some("2016-01-01_01-00-00_000000Z"),
        Some("2016-01-01_01-00-00_000000Z"),
    );

    // Recent tombstone: deleted 2026-01-01, within a 3650-day window from 2026-06-08.
    insert_row(
        &db,
        "rec00000002",
        "0sentinel00",
        "recent_del.txt",
        "2026-01-01_00-00-00_000000Z",
        200,
        Some("2026-01-01_01-00-00_000000Z"),
        Some("2026-01-01_01-00-00_000000Z"),
    );

    // Old stale live row: last_seen 2016-01-01, no deleted_time, not visited this run.
    insert_row(
        &db,
        "stl00000003",
        "0sentinel00",
        "stale_live.txt",
        "2016-01-01_00-00-00_000000Z",
        300,
        Some("2016-01-01_01-00-00_000000Z"),
        None,
    );

    let s = make();
    let peer = peer_url(&dir);
    s.open(&peer, false).unwrap();
    (dir, s, peer)
}

#[test]
fn prune_removes_old_tombstone_rows() {
    // 018.1: tombstone rows older than keep_del_days are removed.
    let (_, s, peer) = peer_with_preloaded_rows("prune_old_ts");
    s.prune(&peer, 30).unwrap();
    let row = s.read_row(&peer, "old00000001").unwrap();
    assert!(row.is_none(), "old tombstone row must be removed by prune");
}

#[test]
fn prune_keeps_recent_tombstone_rows() {
    // 018.2: tombstone rows within keep_del_days are kept.
    let (_, s, peer) = peer_with_preloaded_rows("prune_keep_recent");
    // 3650-day window: the 2026-01-01 row (~158 days ago) is well within range.
    s.prune(&peer, 3650).unwrap();
    let row = s.read_row(&peer, "rec00000002").unwrap();
    assert!(row.is_some(), "recent tombstone row must be kept within the window");
}

#[test]
fn prune_removes_stale_live_rows() {
    // 018.3: stale live row (deleted_time NULL, last_seen old, not visited) is removed.
    let (_, s, peer) = peer_with_preloaded_rows("prune_stale_live");
    s.prune(&peer, 30).unwrap();
    let row = s.read_row(&peer, "stl00000003").unwrap();
    assert!(row.is_none(), "stale live row must be removed by prune");
}

// ---- 016.13-016.18: SWAP recovery applied during open ----

#[test]
fn recovery_old_and_snapshot_exist_removes_old() {
    // 016.14: old + snapshot.db → delete new (if present) + delete old; snapshot.db stays.
    let dir = test_peer("rec_old_tgt");
    let snap = db_path(&dir);
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    create_snapshot_db(&snap);
    create_snapshot_db(&swap.join("old"));
    create_snapshot_db(&swap.join("new"));

    let s = make();
    let peer = peer_url(&dir);
    s.open(&peer, false).unwrap();

    assert!(!swap.join("old").exists(), "old must be removed after recovery");
    assert!(!swap.join("new").exists(), "new must be removed after recovery");
    assert!(snap.exists(), "snapshot.db must remain");
}

#[test]
fn recovery_old_new_no_target_promotes_new_to_snapshot() {
    // 016.15: old + new + no snapshot.db → rename new to snapshot.db, delete old.
    let dir = test_peer("rec_old_new_no_tgt");
    let snap = db_path(&dir);
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    create_snapshot_db(&swap.join("old"));
    create_snapshot_db(&swap.join("new"));

    let s = make();
    s.open(&peer_url(&dir), false).unwrap();

    assert!(snap.exists(), "new must have been promoted to snapshot.db");
    assert!(!swap.join("old").exists(), "old must be deleted");
    assert!(!swap.join("new").exists(), "new must no longer be in SWAP");
}

#[test]
fn recovery_old_no_new_no_target_renames_old_to_snapshot() {
    // 016.16: old + no new + no snapshot.db → rename old to snapshot.db.
    let dir = test_peer("rec_old_no_new");
    let snap = db_path(&dir);
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    create_snapshot_db(&swap.join("old"));

    let s = make();
    s.open(&peer_url(&dir), false).unwrap();

    assert!(snap.exists(), "old must have been renamed to snapshot.db");
    assert!(!swap.join("old").exists(), "old must no longer be in SWAP");
}

#[test]
fn recovery_no_old_new_and_target_deletes_new() {
    // 016.17: no old + new + snapshot.db exists → delete new; snapshot.db kept.
    let dir = test_peer("rec_no_old_new_tgt");
    let snap = db_path(&dir);
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    create_snapshot_db(&snap);
    create_snapshot_db(&swap.join("new"));

    let s = make();
    s.open(&peer_url(&dir), false).unwrap();

    assert!(!swap.join("new").exists(), "stale new must be deleted");
    assert!(snap.exists(), "snapshot.db must remain unchanged");
}

#[test]
fn recovery_no_old_new_no_target_promotes_new() {
    // 016.18: no old + new + no snapshot.db → rename new to snapshot.db.
    let dir = test_peer("rec_no_old_new_only");
    let snap = db_path(&dir);
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    create_snapshot_db(&swap.join("new"));

    let s = make();
    s.open(&peer_url(&dir), false).unwrap();

    assert!(snap.exists(), "new must have been promoted to snapshot.db");
    assert!(!swap.join("new").exists(), "new must no longer be in SWAP");
}

// ---- 024.2: dry-run skips peer-side SWAP recovery ----

#[test]
fn dry_run_open_skips_swap_recovery() {
    // 024.2: --dry-run skips peer-side SWAP recovery at startup.
    // State: old present, snapshot.db missing; normal run would rename old to snapshot.db.
    // Dry-run must leave old untouched.
    let dir = test_peer("dryrun_no_recovery");
    let swap = dir.join(".kitchensync/SWAP/snapshot.db");
    create_snapshot_db(&swap.join("old"));

    let s = make();
    let peer = peer_url(&dir);
    s.open(&peer, true).unwrap(); // dry-run open

    assert!(
        swap.join("old").exists(),
        "dry-run must not apply SWAP recovery (old must remain)"
    );
}

// ---- 024.3: dry-run downloads live snapshot.db as-is ----

#[test]
fn dry_run_open_downloads_live_snapshot() {
    // 024.3: --dry-run downloads each reachable peer's live snapshot.db as-is.
    let (dir, s1, peer1) = open_peer("dryrun_live_src");
    let id = s1.path_identity("live.txt");
    let pid = s1.path_identity("/");
    let mt = s1.now();
    s1.record_present(&peer1, &id, &pid, "live.txt", &mt, 9999).unwrap();
    s1.writeback(&peer1, false).unwrap(); // commit data to the peer via normal writeback

    // Re-open same peer in dry-run mode; must see the committed row from the live snapshot.
    let s2 = make();
    s2.open(&peer1, true).unwrap();
    let row = s2
        .read_row(&peer1, &id)
        .unwrap()
        .expect("live data must be readable after dry-run open");
    assert_eq!(row.byte_size, 9999);
}

// ---- 024.6: dry-run creates and updates local temp DB ----

#[test]
fn dry_run_creates_and_updates_local_temp_db() {
    // 024.6: --dry-run creates and updates the local temp snapshot databases.
    let dir = test_peer("dryrun_local_upd");
    let s = make();
    let peer = peer_url(&dir);
    s.open(&peer, true).unwrap(); // dry-run open creates local temp DB
    let id = s.path_identity("temp.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "temp.txt", &mt, 111).unwrap();
    let row = s
        .read_row(&peer, &id)
        .unwrap()
        .expect("local temp DB must be updatable during dry-run");
    assert_eq!(row.byte_size, 111);
}

// ---- 024.18: dry-run skips writeback upload ----

#[test]
fn dry_run_writeback_does_not_upload() {
    // 024.18: --dry-run does not upload updated local temp snapshots back to peers.
    let dir = test_peer("dryrun_no_upload");
    let s = make();
    let peer = peer_url(&dir);
    s.open(&peer, true).unwrap();
    let id = s.path_identity("nowrite.txt");
    let pid = s.path_identity("/");
    let mt = s.now();
    s.record_present(&peer, &id, &pid, "nowrite.txt", &mt, 55).unwrap();
    s.writeback(&peer, true).unwrap(); // dry-run writeback must not write to peer

    // Peer started with no snapshot.db; dry-run skips upload so it must still not exist.
    assert!(
        !db_path(&dir).exists(),
        "dry-run writeback must not create snapshot.db on the peer"
    );
}

// ---- 014.12, 014.13: sentinel parent, root not tracked ----

#[test]
fn read_row_returns_none_for_sync_root() {
    // 014.13: the sync root itself is never tracked; only its children are.
    let (_, s, peer) = open_peer("root_no_row");
    let root_id = s.path_identity("/");
    assert!(
        s.read_row(&peer, &root_id).unwrap().is_none(),
        "sync root must never have a snapshot row"
    );
}

#[test]
fn root_level_entry_uses_sentinel_as_parent() {
    // 014.12: root-level entry's parent_id is path_identity("/").
    let (_, s, peer) = open_peer("root_parent");
    let sentinel = s.path_identity("/");
    let id = s.path_identity("toplevel.txt");
    let mt = s.now();
    s.record_present(&peer, &id, &sentinel, "toplevel.txt", &mt, 42).unwrap();
    let row = s.read_row(&peer, &id).unwrap().expect("row must exist");
    assert_eq!(row.parent_id, sentinel, "root-level entry must use sentinel as parent_id");
}
