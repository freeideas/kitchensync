//! Black-box integration tests for transport_sftpbackend.
//!
//! Tests drive the crate only through the public SftpBackend and SftpConnection
//! traits. Each test that mutates the HOME or SSH_AUTH_SOCK environment
//! variables holds ENV_LOCK for the duration of the connect call so that the
//! parallel test harness cannot race on global env state.
//!
//! An ephemeral SFTP server (extart/ephemeral-sftp-server.py, launched via
//! the bundled uv) provides a real SFTP endpoint so that every test exercises
//! the full SSH stack on 127.0.0.1.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use transport_sftpbackend::{BackendError, SftpBackend, SftpConnection};

// All tests that write HOME or SSH_AUTH_SOCK hold this lock for the lifetime
// of any connect() call so that the parallel test harness does not race on
// global environment state.
static ENV_LOCK: Mutex<()> = Mutex::new(());

// ── ephemeral SFTP server ──────────────────────────────────────────────────

struct ServerHandle {
    child: Child,
    port: u16,
    host_key_type: String,
    host_key_b64: String,
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Return the kitchensync workspace root.
///
/// CARGO_MANIFEST_DIR for this crate is
/// `.../kitchensync/proj/subpjx/Transport/subpjx/SftpBackend`.
/// Counting ancestors: [0] SftpBackend, [1] subpjx, [2] Transport, [3] subpjx,
/// [4] proj, [5] kitchensync.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(5)
        .unwrap()
        .to_path_buf()
}

/// Launch the ephemeral SFTP server with `extra_args`, wait for it to print
/// its port on stdout and its host key on stderr, then return the handle.
fn start_server(extra_args: &[&str]) -> ServerHandle {
    let root = workspace_root();
    let uv = root.join("aitc/bin/uv.linux");
    let script = root.join("extart/ephemeral-sftp-server.py");

    let mut cmd = Command::new(&uv);
    cmd.arg("run").arg("--script").arg(&script);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Do not pass the caller's SSH_AUTH_SOCK into the server process.
        .env_remove("SSH_AUTH_SOCK");

    let mut child = cmd.spawn().expect("failed to spawn ephemeral SFTP server");

    // Drain stderr in a background thread and capture the "host key: ..." line.
    let stderr = child.stderr.take().unwrap();
    let (hk_tx, hk_rx) = std::sync::mpsc::channel::<(String, String)>();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            if let Some(rest) = line.strip_prefix("host key: ") {
                let mut parts = rest.splitn(2, ' ');
                let t = parts.next().unwrap_or("").to_string();
                let b = parts.next().unwrap_or("").to_string();
                let _ = hk_tx.send((t, b));
                // Keep draining; early return would close the read end and SIGPIPE the server.
            }
        }
    });

    // The server prints the port as the only stdout line before entering its
    // accept loop, so reading it tells us the server is ready.
    let stdout = child.stdout.take().unwrap();
    let mut port_line = String::new();
    BufReader::new(stdout)
        .read_line(&mut port_line)
        .expect("port line from ephemeral server");
    let port: u16 = port_line.trim().parse().expect("server port number");

    let (hkt, hkb) = hk_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("host key from server stderr");

    ServerHandle {
        child,
        port,
        host_key_type: hkt,
        host_key_b64: hkb,
    }
}

// ── HOME and known_hosts ───────────────────────────────────────────────────

/// Reset the shared test HOME to a clean slate and point the process HOME at
/// it. Must be called while ENV_LOCK is held by the caller.
#[allow(unused_unsafe)]
fn setup_home() -> PathBuf {
    let home = std::env::temp_dir().join("sftp_test_home_ks");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    // SAFETY: ENV_LOCK is held by all callers; no other thread reads HOME
    // concurrently while we hold the lock.
    unsafe { std::env::set_var("HOME", &home) };
    home
}

/// Write a known_hosts entry for `server` into `home/.ssh/known_hosts`.
fn write_known_hosts(home: &Path, server: &ServerHandle) {
    let entry = format!(
        "[127.0.0.1]:{} {} {}\n",
        server.port, server.host_key_type, server.host_key_b64
    );
    std::fs::write(home.join(".ssh/known_hosts"), entry).unwrap();
}

// ── SSH key generation ─────────────────────────────────────────────────────

fn run_keygen(args: &[&str], out_path: &Path) {
    let _ = std::fs::remove_file(out_path);
    let pub_path = PathBuf::from(format!("{}.pub", out_path.display()));
    let _ = std::fs::remove_file(&pub_path);
    let s = Command::new("ssh-keygen")
        .args(args)
        .arg(out_path)
        .status()
        .expect("ssh-keygen must be installed for key-auth tests");
    assert!(s.success(), "ssh-keygen failed for {:?}", out_path);
}

fn gen_ed25519(ssh_dir: &Path) -> PathBuf {
    let p = ssh_dir.join("id_ed25519");
    run_keygen(&["-t", "ed25519", "-N", "", "-q", "-f"], &p);
    p
}

fn gen_ecdsa(ssh_dir: &Path) -> PathBuf {
    let p = ssh_dir.join("id_ecdsa");
    run_keygen(&["-t", "ecdsa", "-b", "256", "-N", "", "-q", "-f"], &p);
    p
}

fn gen_rsa(ssh_dir: &Path) -> PathBuf {
    let p = ssh_dir.join("id_rsa");
    run_keygen(&["-t", "rsa", "-b", "2048", "-N", "", "-q", "-f"], &p);
    p
}

fn pub_key_path(priv_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.pub", priv_path.display()))
}

// ── URL builder ───────────────────────────────────────────────────────────

fn make_url(port: u16, user: &str, password: Option<&str>, root: &str, query: Option<&str>) -> String {
    let creds = match password {
        Some(pw) => format!("{}:{}", user, pw),
        None => user.to_string(),
    };
    let q = match query {
        Some(s) => format!("?{}", s),
        None => String::new(),
    };
    format!("sftp://{}@127.0.0.1:{}{}{}", creds, port, root, q)
}

fn backend() -> Arc<dyn SftpBackend> {
    transport_sftpbackend::new()
}

// ── shared fixture for file-operation tests ───────────────────────────────

/// Start a password-auth server, set up HOME with its known_hosts entry, and
/// connect. Returns (server_handle_kept_alive, connection). The caller must
/// hold ENV_LOCK for the duration so HOME stays consistent.
fn setup_conn(root: &str) -> (ServerHandle, Arc<dyn SftpConnection>) {
    let server = start_server(&["--password", "pw"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    let url = make_url(server.port, "alice", Some("pw"), root, None);
    let conn = backend()
        .connect(&url, Duration::from_secs(30), false)
        .expect("setup_conn: connect must succeed");
    (server, conn)
}

// ═════════════════════════════════════════════════════════════════════════════
// Connection tests (auth, host verification, timeout, root handling)
// ═════════════════════════════════════════════════════════════════════════════

// 004.1, 004.8 -- inline URL password is the first credential source;
// a host whose key matches its known_hosts entry passes verification.
#[test]
fn connect_password_auth_succeeds_with_matching_host_key() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = start_server(&["--password", "testpass"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    let url = make_url(server.port, "alice", Some("testpass"), "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(result.is_ok(), "password auth with matching host key must succeed: {:?}", result.err());
}

// 004.9 -- a host absent from known_hosts is rejected.
#[test]
fn connect_unknown_host_is_rejected() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = start_server(&["--password", "pw"]);
    let home = setup_home();
    // known_hosts file exists but has no entry for this server.
    std::fs::write(home.join(".ssh/known_hosts"), "").unwrap();
    let url = make_url(server.port, "alice", Some("pw"), "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert_eq!(
        result.err(),
        Some(BackendError::PermissionDenied),
        "unknown host must be rejected with PermissionDenied"
    );
}

// 004.10 -- an inline SFTP password containing percent-encoded characters is
// percent-decoded before authentication (%40 -> @, %3A -> :).
#[test]
fn connect_percent_decoded_password_is_used() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Actual password contains '@' and ':'.
    let server = start_server(&["--password", "p@ss:word"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    // URL encodes '@' as %40 and ':' as %3A.
    let url = make_url(server.port, "alice", Some("p%40ss%3Aword"), "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(result.is_ok(), "percent-decoded password must authenticate: {:?}", result.as_ref().err());
}

// 004.3, required auth coverage -- ~/.ssh/id_ed25519 is the third credential
// source; the client must be able to connect when the server accepts only an
// Ed25519 public key and no password/agent is available.
#[test]
fn connect_ed25519_key_auth_succeeds() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = setup_home();
    let ssh_dir = home.join(".ssh");
    let priv_path = gen_ed25519(&ssh_dir);
    let pub_path = pub_key_path(&priv_path);
    let server = start_server(&["--authorized-key", pub_path.to_str().unwrap()]);
    write_known_hosts(&home, &server);
    // Ensure SSH_AUTH_SOCK is absent so the agent source is skipped.
    unsafe { std::env::remove_var("SSH_AUTH_SOCK") };
    // URL has no inline password; no id_ecdsa or id_rsa in ssh_dir.
    let url = make_url(server.port, "alice", None, "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(result.is_ok(), "id_ed25519 auth must succeed: {:?}", result.as_ref().err());
}

// 004.4 -- ~/.ssh/id_ecdsa is the fourth credential source, used when
// id_ed25519 is absent.
#[test]
fn connect_ecdsa_key_auth_succeeds_when_ed25519_absent() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = setup_home();
    let ssh_dir = home.join(".ssh");
    // Only id_ecdsa is present; id_ed25519 is absent.
    let priv_path = gen_ecdsa(&ssh_dir);
    let pub_path = pub_key_path(&priv_path);
    let server = start_server(&["--authorized-key", pub_path.to_str().unwrap()]);
    write_known_hosts(&home, &server);
    unsafe { std::env::remove_var("SSH_AUTH_SOCK") };
    let url = make_url(server.port, "alice", None, "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(
        result.is_ok(),
        "id_ecdsa auth must succeed when id_ed25519 is absent: {:?}",
        result.as_ref().err()
    );
}

// 004.5 -- ~/.ssh/id_rsa is the fifth credential source, used when id_ed25519
// and id_ecdsa are both absent.
#[test]
fn connect_rsa_key_auth_succeeds_when_ed25519_and_ecdsa_absent() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = setup_home();
    let ssh_dir = home.join(".ssh");
    // Only id_rsa is present.
    let priv_path = gen_rsa(&ssh_dir);
    let pub_path = pub_key_path(&priv_path);
    let server = start_server(&["--authorized-key", pub_path.to_str().unwrap()]);
    write_known_hosts(&home, &server);
    unsafe { std::env::remove_var("SSH_AUTH_SOCK") };
    let url = make_url(server.port, "alice", None, "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(
        result.is_ok(),
        "id_rsa auth must succeed when id_ed25519 and id_ecdsa are absent: {:?}",
        result.as_ref().err()
    );
}

// 004.6 -- a credential source that is absent is skipped; the next source is
// attempted. (No inline password; no SSH agent; no id_ecdsa or id_rsa; only
// id_ed25519 is present and the server accepts it.)
#[test]
fn connect_absent_sources_are_skipped_and_next_is_tried() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = setup_home();
    let ssh_dir = home.join(".ssh");
    let priv_path = gen_ed25519(&ssh_dir);
    let pub_path = pub_key_path(&priv_path);
    // Server is key-only; the password source is absent (no password in URL)
    // and the agent source is absent (no SSH_AUTH_SOCK); id_ecdsa and id_rsa
    // are absent; id_ed25519 is present and must succeed.
    let server = start_server(&["--authorized-key", pub_path.to_str().unwrap()]);
    write_known_hosts(&home, &server);
    unsafe { std::env::remove_var("SSH_AUTH_SOCK") };
    let url = make_url(server.port, "alice", None, "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(
        result.is_ok(),
        "absent sources must be skipped and id_ed25519 must succeed: {:?}",
        result.as_ref().err()
    );
}

// 004.7 -- a credential source rejected by the host causes the next source to
// be attempted. (Wrong inline password rejected; SSH agent fails; id_ed25519
// is accepted.)
#[test]
fn connect_rejected_credential_falls_through_to_next_source() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = setup_home();
    let ssh_dir = home.join(".ssh");
    let priv_path = gen_ed25519(&ssh_dir);
    let pub_path = pub_key_path(&priv_path);
    // Server is key-only: it rejects passwords and accepts only the ed25519 key.
    let server = start_server(&["--authorized-key", pub_path.to_str().unwrap()]);
    write_known_hosts(&home, &server);
    // SSH_AUTH_SOCK points to a nonexistent socket so the agent attempt fails.
    unsafe { std::env::set_var("SSH_AUTH_SOCK", "/tmp/sftp_test_nonexistent_agent_ks") };
    // URL has a wrong password (rejected) then falls through agent (fails)
    // then falls through to id_ed25519 (accepted).
    let url = make_url(server.port, "alice", Some("wrongpass"), "/root", None);
    let result = backend().connect(&url, Duration::from_secs(30), false);
    assert!(
        result.is_ok(),
        "rejected password and failed agent must fall through to id_ed25519: {:?}",
        result.as_ref().err()
    );
}

// 005.6, 005.8 -- --timeout-conn bounds the SSH handshake; a URL whose
// handshake does not complete within the timeout is abandoned and fails.
#[test]
fn connect_timeout_bounds_handshake_and_fails_on_expiry() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // HOME must be set (home_dir() is called during connect) but known_hosts
    // does not matter because the connection fails before host-key checks.
    setup_home();

    // A TCP listener that accepts the connection but never sends the SSH banner.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((conn, _)) = listener.accept() {
            std::thread::sleep(Duration::from_secs(60));
            drop(conn);
        }
    });

    let url = format!("sftp://alice@127.0.0.1:{}/root", port);
    let t0 = Instant::now();
    let result = backend().connect(&url, Duration::from_secs(2), false);
    let elapsed = t0.elapsed();

    assert!(result.is_err(), "timed-out handshake must return an error");
    assert!(
        elapsed < Duration::from_secs(10),
        "handshake timeout must not block longer than 10 s (elapsed {:?})",
        elapsed
    );
}

// 005.7 -- a URL's timeout-conn query parameter overrides --timeout-conn for
// that URL's SSH handshake.
#[test]
fn connect_url_timeout_conn_overrides_default() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    setup_home();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((conn, _)) = listener.accept() {
            std::thread::sleep(Duration::from_secs(60));
            drop(conn);
        }
    });

    // Default timeout is 600 s (would block forever in practice); URL overrides
    // to 2 s so the call must return quickly.
    let url = format!("sftp://alice@127.0.0.1:{}/root?timeout-conn=2", port);
    let t0 = Instant::now();
    let result = backend().connect(&url, Duration::from_secs(600), false);
    let elapsed = t0.elapsed();

    assert!(result.is_err(), "URL timeout-conn override must cause failure");
    assert!(
        elapsed < Duration::from_secs(15),
        "URL timeout-conn=2 must expire in well under 15 s (elapsed {:?})",
        elapsed
    );
}

// 005.10 -- in a normal run, a missing peer root directory is created.
#[test]
fn connect_creates_missing_root_in_normal_run() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = start_server(&["--password", "pw"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    // Root /newroot does not exist on the server yet.
    let url = make_url(server.port, "alice", Some("pw"), "/newroot", None);
    let conn = backend()
        .connect(&url, Duration::from_secs(30), false)
        .expect("connect must create missing root directory");
    let meta = conn.stat("/newroot").unwrap();
    assert!(meta.is_dir, "created root must be a directory");
}

// 005.11 -- in a normal run, missing parent directories of the peer root are
// created.
#[test]
fn connect_creates_missing_root_parents_in_normal_run() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = start_server(&["--password", "pw"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    // Root with multiple missing parent levels.
    let url = make_url(server.port, "alice", Some("pw"), "/a/b/c/root", None);
    let conn = backend()
        .connect(&url, Duration::from_secs(30), false)
        .expect("connect must create root and all missing parents");
    let meta = conn.stat("/a/b/c/root").unwrap();
    assert!(meta.is_dir, "created root (with parents) must be a directory");
}

// 005.13, 024.11 -- in --dry-run, a missing peer root directory is not
// created; the peer is treated as failed for that run.
#[test]
fn connect_dryrun_does_not_create_missing_root() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = start_server(&["--password", "pw"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    let url = make_url(server.port, "alice", Some("pw"), "/dryrunroot", None);
    // First dry-run call: must fail because root is absent.
    assert!(
        backend().connect(&url, Duration::from_secs(30), true).is_err(),
        "dry-run must fail when root is missing"
    );
    // Second dry-run call to the same path: must still fail, proving the first
    // call did not create the root directory.
    assert!(
        backend().connect(&url, Duration::from_secs(30), true).is_err(),
        "root must not have been created by the first dry-run call"
    );
}

// 005.14, 024.11 -- in --dry-run, a URL whose root does not already exist is
// treated as failed for that run.
#[test]
fn connect_dryrun_missing_root_fails() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = start_server(&["--password", "pw"]);
    let home = setup_home();
    write_known_hosts(&home, &server);
    let url = make_url(server.port, "alice", Some("pw"), "/nosuchroot", None);
    let result = backend().connect(&url, Duration::from_secs(30), true);
    assert!(result.is_err(), "dry-run with missing root must be treated as failed");
}

// ═════════════════════════════════════════════════════════════════════════════
// File operation tests (022.x)
// Each test establishes a fresh password-auth connection and exercises one
// behavior over SFTP. All paths are absolute and scoped to the root dir.
// ═════════════════════════════════════════════════════════════════════════════

// 022.2, 022.3 -- list_dir returns each immediate child's name, is_dir,
// mod_time, and byte_size; byte_size is the file size in bytes for a regular
// file.
#[test]
fn list_dir_returns_entry_fields_for_regular_file() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/listdir_file");
    let wh = conn.open_write("/listdir_file/data.txt").unwrap();
    conn.write(&wh, b"hello world").unwrap();
    conn.close_write(wh).unwrap();
    let entries = conn.list_dir("/listdir_file").unwrap();
    let entry = entries.iter().find(|e| e.name == "data.txt").expect("file entry must appear");
    assert!(!entry.is_dir, "regular file must have is_dir == false");
    assert_eq!(entry.byte_size, 11, "byte_size must equal the file size in bytes");
    let _ = entry.mod_time; // readable; no specific value required by spec
}

// 022.4 -- list_dir reports byte_size as -1 for a directory.
#[test]
fn list_dir_byte_size_is_negative_one_for_directory() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/listdir_dir");
    conn.create_dir("/listdir_dir/adir").unwrap();
    let entries = conn.list_dir("/listdir_dir").unwrap();
    let entry = entries.iter().find(|e| e.name == "adir").expect("dir entry must appear");
    assert!(entry.is_dir, "directory entry must have is_dir == true");
    assert_eq!(entry.byte_size, -1, "directory byte_size must be -1");
}

// 022.5 -- stat returns mod_time, byte_size, and is_dir for an existing
// regular file.
#[test]
fn stat_returns_metadata_for_regular_file() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/stat_file");
    let wh = conn.open_write("/stat_file/f.txt").unwrap();
    conn.write(&wh, b"hello").unwrap();
    conn.close_write(wh).unwrap();
    let meta = conn.stat("/stat_file/f.txt").unwrap();
    assert!(!meta.is_dir, "regular file must have is_dir == false");
    assert_eq!(meta.byte_size, 5, "byte_size must equal the file size");
    let _ = meta.mod_time;
}

// 022.5 -- stat returns metadata for an existing directory.
#[test]
fn stat_returns_metadata_for_directory() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/stat_dir");
    conn.create_dir("/stat_dir/mydir").unwrap();
    let meta = conn.stat("/stat_dir/mydir").unwrap();
    assert!(meta.is_dir, "directory must have is_dir == true");
    assert_eq!(meta.byte_size, -1, "directory byte_size must be -1");
}

// 022.6, 022.17 -- stat returns NotFound when the path does not exist.
#[test]
fn stat_returns_not_found_for_missing_path() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/stat_notfound");
    let result = conn.stat("/stat_notfound/no_such.txt");
    assert_eq!(
        result,
        Err(BackendError::NotFound),
        "missing path must return NotFound"
    );
}

// 022.7 -- read(handle, max_bytes) returns the next chunk of bytes; an empty
// chunk signals EOF at the end of the file.
#[test]
fn read_returns_chunks_and_empty_vec_at_eof() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/read_eof");
    let content: &[u8] = b"abcdefghij";
    let wh = conn.open_write("/read_eof/data.bin").unwrap();
    conn.write(&wh, content).unwrap();
    conn.close_write(wh).unwrap();

    let rh = conn.open_read("/read_eof/data.bin").unwrap();
    let mut collected = Vec::new();
    loop {
        let chunk = conn.read(&rh, 4).unwrap();
        if chunk.is_empty() {
            break;
        }
        collected.extend_from_slice(&chunk);
    }
    conn.close_read(rh).unwrap();
    assert_eq!(collected, content, "read must return the full file content");
}

// 022.8 -- open_write creates the target file and any missing parent
// directories.
#[test]
fn open_write_creates_file_and_missing_parent_directories() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/write_parents");
    let wh = conn.open_write("/write_parents/a/b/out.txt").unwrap();
    conn.write(&wh, b"written").unwrap();
    conn.close_write(wh).unwrap();

    // Verify by reading back.
    let rh = conn.open_read("/write_parents/a/b/out.txt").unwrap();
    let mut data = Vec::new();
    loop {
        let chunk = conn.read(&rh, 64).unwrap();
        if chunk.is_empty() {
            break;
        }
        data.extend_from_slice(&chunk);
    }
    conn.close_read(rh).unwrap();
    assert_eq!(data, b"written", "file written through nested parents must be readable");
}

// 022.9 -- create_dir creates the directory and any missing parent
// directories.
#[test]
fn create_dir_creates_directory_and_missing_parents() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/create_dir_test");
    conn.create_dir("/create_dir_test/x/y/z").unwrap();
    let meta = conn.stat("/create_dir_test/x/y/z").unwrap();
    assert!(meta.is_dir, "created nested directory must be a directory");
}

// 022.10 -- rename(src, dst) moves src to dst when dst does not exist.
#[test]
fn rename_moves_src_to_dst_when_dst_does_not_exist() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/rename_ok");
    let wh = conn.open_write("/rename_ok/src.txt").unwrap();
    conn.write(&wh, b"data").unwrap();
    conn.close_write(wh).unwrap();

    conn.rename("/rename_ok/src.txt", "/rename_ok/dst.txt").unwrap();

    assert_eq!(
        conn.stat("/rename_ok/src.txt"),
        Err(BackendError::NotFound),
        "src must be gone after rename"
    );
    assert!(
        conn.stat("/rename_ok/dst.txt").is_ok(),
        "dst must exist after rename"
    );
}

// 022.11 -- rename(src, dst) fails when dst already exists.
#[test]
fn rename_fails_when_dst_already_exists() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/rename_fail");
    for name in &["/rename_fail/src.txt", "/rename_fail/dst.txt"] {
        let wh = conn.open_write(name).unwrap();
        conn.write(&wh, b"x").unwrap();
        conn.close_write(wh).unwrap();
    }
    let result = conn.rename("/rename_fail/src.txt", "/rename_fail/dst.txt");
    assert!(result.is_err(), "rename must fail when dst already exists");
}

// 022.12 -- delete_file removes a file.
#[test]
fn delete_file_removes_a_file() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/delete_file_test");
    let wh = conn.open_write("/delete_file_test/bye.txt").unwrap();
    conn.write(&wh, b"bye").unwrap();
    conn.close_write(wh).unwrap();

    conn.delete_file("/delete_file_test/bye.txt").unwrap();
    assert_eq!(
        conn.stat("/delete_file_test/bye.txt"),
        Err(BackendError::NotFound),
        "deleted file must not exist"
    );
}

// 022.13 -- delete_dir removes an empty directory.
#[test]
fn delete_dir_removes_an_empty_directory() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/delete_dir_test");
    conn.create_dir("/delete_dir_test/emptydir").unwrap();
    conn.delete_dir("/delete_dir_test/emptydir").unwrap();
    assert_eq!(
        conn.stat("/delete_dir_test/emptydir"),
        Err(BackendError::NotFound),
        "deleted directory must not exist"
    );
}

// 022.14 -- set_mod_time sets the modification time of a regular file.
#[test]
fn set_mod_time_sets_modification_time_of_a_file() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/modtime_file");
    let wh = conn.open_write("/modtime_file/f.txt").unwrap();
    conn.write(&wh, b"x").unwrap();
    conn.close_write(wh).unwrap();

    let target = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    conn.set_mod_time("/modtime_file/f.txt", target).unwrap();
    let meta = conn.stat("/modtime_file/f.txt").unwrap();
    let diff = time_diff(meta.mod_time, target);
    assert!(
        diff <= Duration::from_secs(2),
        "mod_time must be within 2 s of the target (diff {:?})",
        diff
    );
}

// 022.14 -- set_mod_time sets the modification time of a directory.
#[test]
fn set_mod_time_sets_modification_time_of_a_directory() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/modtime_dir");
    conn.create_dir("/modtime_dir/timed_dir").unwrap();

    let target = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    conn.set_mod_time("/modtime_dir/timed_dir", target).unwrap();
    let meta = conn.stat("/modtime_dir/timed_dir").unwrap();
    let diff = time_diff(meta.mod_time, target);
    assert!(
        diff <= Duration::from_secs(2),
        "directory mod_time must be within 2 s of the target (diff {:?})",
        diff
    );
}

fn time_diff(a: SystemTime, b: SystemTime) -> Duration {
    if a >= b {
        a.duration_since(b).unwrap()
    } else {
        b.duration_since(a).unwrap()
    }
}

// 022.15 -- list_dir includes regular files and directories in the result.
// (Symlink exclusion is not tested here per the testing guidelines which
// prohibit creating symlinks in tests.)
#[test]
fn list_dir_includes_regular_files_and_directories() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/list_regular");
    let wh = conn.open_write("/list_regular/reg.txt").unwrap();
    conn.write(&wh, b"").unwrap();
    conn.close_write(wh).unwrap();
    conn.create_dir("/list_regular/reg_dir").unwrap();

    let entries = conn.list_dir("/list_regular").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"reg.txt"), "regular file must appear in listing");
    assert!(names.contains(&"reg_dir"), "directory must appear in listing");
}

// 022.17 -- every operation reports failures using only the three categories:
// NotFound, PermissionDenied, and Io. (Implicitly exercised across all error
// tests; verified here explicitly by exhaustive match.)
#[test]
fn error_categories_are_limited_to_three_variants() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_s, conn) = setup_conn("/errcats");
    let err = conn.stat("/errcats/nonexistent").unwrap_err();
    match err {
        BackendError::NotFound | BackendError::PermissionDenied | BackendError::Io => {}
    }
}

// 022.18 -- a network failure such as a connection drop surfaces as Io.
#[test]
fn network_failure_surfaces_as_io_error() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (server, conn) = setup_conn("/netfail");
    // Kill the server process, severing the TCP connection.
    drop(server);
    // Give the OS a moment to propagate the connection drop.
    std::thread::sleep(Duration::from_millis(500));
    let result = conn.stat("/netfail/anything");
    assert_eq!(
        result,
        Err(BackendError::Io),
        "a dropped connection must surface as BackendError::Io"
    );
}
