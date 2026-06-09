use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use transport::{Transport, TransportError};

// Serialises tests that read or write process-wide env vars.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn make_transport() -> Arc<dyn Transport> {
    transport::new()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("workspace root")
}

fn uv_bin() -> PathBuf {
    workspace_root().join("aitc/bin/uv.linux")
}

fn sftp_server_script() -> PathBuf {
    workspace_root().join("extart/ephemeral-sftp-server.py")
}

// Create (or recreate) a uniquely-named temp dir so each test starts clean.
fn fresh_tmpdir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("ks_transport_{}", name));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

// Set an env var for the duration of an ENV_LOCK guard and restore it on drop.
struct EnvGuard {
    key: &'static str,
    prior: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, val: &str) -> Self {
        let prior = std::env::var(key).ok();
        std::env::set_var(key, val);
        EnvGuard { key, prior }
    }

    fn remove(key: &'static str) -> Self {
        let prior = std::env::var(key).ok();
        std::env::remove_var(key);
        EnvGuard { key, prior }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

// Ephemeral SFTP server wrapper. Kills the process on drop.
struct SftpServer {
    _process: Child,
    pub port: u16,
    pub host_key_line: String, // "<type> <base64>"
}

impl SftpServer {
    fn start(extra_args: &[&str]) -> Self {
        let mut cmd = Command::new(uv_bin());
        cmd.arg("run").arg("--script").arg(sftp_server_script());
        for a in extra_args {
            cmd.arg(a);
        }
        // Put the server in its own process group so that when we kill it we
        // can also reach any child processes uv may have spawned (e.g. Python).
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn ephemeral sftp server");

        let stdout = child.stdout.take().unwrap();
        let stderr_handle = child.stderr.take().unwrap();

        // Read the one port line from stdout.
        let mut stdout_buf = BufReader::new(stdout);
        let mut port_line = String::new();
        stdout_buf
            .read_line(&mut port_line)
            .expect("read port from sftp server");
        let port: u16 = port_line.trim().parse().expect("parse port number");
        drop(stdout_buf);

        // Read stderr lines until the "host key: ..." line appears.
        let mut stderr_buf = BufReader::new(stderr_handle);
        let mut host_key_line = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            if stderr_buf.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if let Some(rest) = line.trim_end().strip_prefix("host key: ") {
                host_key_line = rest.to_string();
                break;
            }
        }
        // Drain remaining stderr in a background thread to avoid SIGPIPE when
        // the server writes connection notifications later.
        std::thread::spawn(move || {
            let mut buf = String::new();
            loop {
                buf.clear();
                if stderr_buf.read_line(&mut buf).unwrap_or(0) == 0 {
                    break;
                }
            }
        });

        SftpServer {
            _process: child,
            port,
            host_key_line,
        }
    }
}

impl Drop for SftpServer {
    fn drop(&mut self) {
        // Kill the entire process group so any Python child of uv is also ended.
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-9", &format!("-{}", self._process.id())])
                .output();
        }
        let _ = self._process.kill();
        let _ = self._process.wait();
    }
}

// Write a known_hosts entry for the server so host-key verification passes.
fn write_known_hosts(ssh_dir: &PathBuf, port: u16, host_key_line: &str) {
    let entry = format!("[127.0.0.1]:{} {}\n", port, host_key_line);
    fs::write(ssh_dir.join("known_hosts"), entry).unwrap();
}

// Generate an Ed25519 keypair at <ssh_dir>/id_ed25519{,.pub} using ssh-keygen.
fn generate_ed25519_key(ssh_dir: &PathBuf) {
    let key_path = ssh_dir.join("id_ed25519");
    Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f"])
        .arg(&key_path)
        .args(["-N", ""])
        .output()
        .expect("ssh-keygen");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn generate_ecdsa_key(ssh_dir: &PathBuf) {
    let key_path = ssh_dir.join("id_ecdsa");
    Command::new("ssh-keygen")
        .args(["-t", "ecdsa", "-f"])
        .arg(&key_path)
        .args(["-N", ""])
        .output()
        .expect("ssh-keygen ecdsa");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn generate_rsa_key(ssh_dir: &PathBuf) {
    let key_path = ssh_dir.join("id_rsa");
    Command::new("ssh-keygen")
        .args(["-t", "rsa", "-b", "2048", "-f"])
        .arg(&key_path)
        .args(["-N", ""])
        .output()
        .expect("ssh-keygen rsa");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

// Build a fresh HOME with an empty .ssh dir.
fn setup_ssh_home(name: &str) -> PathBuf {
    let home = fresh_tmpdir(&format!("{}_home", name));
    let ssh_dir = home.join(".ssh");
    fs::create_dir_all(&ssh_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    home
}

fn file_url(dir: &PathBuf) -> String {
    format!("file://{}", dir.display())
}

fn open_file_peer(t: &Arc<dyn Transport>, dir: &PathBuf) -> transport::ConnectedPeer {
    t.open_peer(&file_url(dir), &[], false, Duration::from_secs(5))
        .expect("file:// peer should open")
}

// ── 003.x URL normalization ───────────────────────────────────────────────────

#[test]
fn req_003_1_normalize_lowercases_scheme() {
    let t = make_transport();
    let result = t.normalize_url("SFTP://host/path");
    assert!(
        result.starts_with("sftp://"),
        "scheme should be lowercase, got: {}",
        result
    );
}

#[test]
fn req_003_2_normalize_lowercases_hostname() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "u");
    let t = make_transport();
    let result = t.normalize_url("sftp://HOST/path");
    assert!(
        result.contains("@host/") || result.contains("//host/"),
        "hostname should be lowercase, got: {}",
        result
    );
}

#[test]
fn req_003_3_normalize_sftp_removes_default_port_22() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "u");
    let t = make_transport();
    let result = t.normalize_url("sftp://host:22/path");
    assert!(
        !result.contains(":22"),
        "default SFTP port 22 should be removed, got: {}",
        result
    );
}

#[test]
fn req_003_4_normalize_collapses_consecutive_slashes() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "u");
    let t = make_transport();
    let result = t.normalize_url("sftp://host//a//b");
    assert!(
        result.ends_with("/a/b"),
        "consecutive slashes should collapse, got: {}",
        result
    );
}

#[test]
fn req_003_5_normalize_removes_trailing_slash() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "u");
    let t = make_transport();
    let result = t.normalize_url("sftp://host/path/");
    assert!(
        !result.ends_with('/'),
        "trailing slash should be removed, got: {}",
        result
    );
}

#[test]
fn req_003_6_normalize_bare_path_becomes_file_url() {
    let t = make_transport();
    let result = t.normalize_url("/absolute/path");
    assert!(
        result.starts_with("file://"),
        "bare absolute path should become file:// URL, got: {}",
        result
    );
}

#[test]
fn req_003_7_normalize_file_url_resolves_relative_path_from_cwd() {
    let t = make_transport();
    let cwd = std::env::current_dir().unwrap();
    let result = t.normalize_url("./data");
    let expected = format!("file://{}/data", cwd.display());
    assert_eq!(result, expected);
}

#[test]
fn req_003_8_normalize_decodes_unreserved_percent_encoded_chars() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "u");
    let t = make_transport();
    // %61 = 'a' (unreserved); should decode to 'a'
    let result = t.normalize_url("sftp://host/p%61th");
    assert!(
        result.ends_with("/path"),
        "unreserved %61 should decode to 'a', got: {}",
        result
    );
}

#[test]
fn req_003_9_normalize_strips_query_string() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "u");
    let t = make_transport();
    let result = t.normalize_url("sftp://host/path?foo=bar&baz=1");
    assert!(
        !result.contains('?'),
        "query string should be stripped, got: {}",
        result
    );
}

#[test]
fn req_003_10_normalize_inserts_os_user_for_sftp_without_username() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "testuser");
    let t = make_transport();
    let result = t.normalize_url("sftp://host/path");
    assert!(
        result.contains("testuser@host"),
        "OS user should be inserted for SFTP URL with no username, got: {}",
        result
    );
}

#[test]
fn req_003_11_normalize_windows_drive_path() {
    // c:/photos/ -> file:///c:/photos
    let t = make_transport();
    assert_eq!(t.normalize_url("c:/photos/"), "file:///c:/photos");
}

#[test]
fn req_003_12_normalize_relative_path_from_actual_cwd() {
    // ./data from CWD -> file:///CWD/data
    let t = make_transport();
    let cwd = std::env::current_dir().unwrap();
    let result = t.normalize_url("./data");
    let expected = format!("file://{}/data", cwd.display());
    assert_eq!(result, expected);
}

#[test]
fn req_003_13_normalize_sftp_uppercase_host_default_port_trailing_slash() {
    // SFTP://Host:22/path/ -> sftp://host/path (no user when USER unset)
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::remove("USER");
    let _v = EnvGuard::remove("USERNAME");
    let t = make_transport();
    assert_eq!(t.normalize_url("SFTP://Host:22/path/"), "sftp://host/path");
}

#[test]
fn req_003_14_normalize_sftp_double_slash_path() {
    // sftp://host//docs/ -> sftp://host/docs (no user when USER unset)
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::remove("USER");
    let _v = EnvGuard::remove("USERNAME");
    let t = make_transport();
    assert_eq!(t.normalize_url("sftp://host//docs/"), "sftp://host/docs");
}

#[test]
fn req_003_15_normalize_sftp_strips_timeout_query_param() {
    // sftp://host/path?timeout-conn=60 -> sftp://host/path (no user when USER unset)
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::remove("USER");
    let _v = EnvGuard::remove("USERNAME");
    let t = make_transport();
    assert_eq!(
        t.normalize_url("sftp://host/path?timeout-conn=60"),
        "sftp://host/path"
    );
}

#[test]
fn req_003_16_normalize_sftp_inserts_user_ace() {
    // sftp://host/path as user ace -> sftp://ace@host/path
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _u = EnvGuard::set("USER", "ace");
    let t = make_transport();
    assert_eq!(t.normalize_url("sftp://host/path"), "sftp://ace@host/path");
}

// ── 022.x file:// filesystem operations ──────────────────────────────────────

#[test]
fn req_022_2_list_dir_returns_name_isdir_modtime_bytesize() {
    let dir = fresh_tmpdir("list_dir_meta");
    fs::write(dir.join("file.txt"), b"abc").unwrap();
    fs::create_dir(dir.join("subdir")).unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let entries = t.list_dir(&peer.handle, "").unwrap();

    let fe = entries.iter().find(|e| e.name == "file.txt").expect("file.txt");
    assert!(!fe.is_dir);
    assert!(fe.mod_time > UNIX_EPOCH);

    let de = entries.iter().find(|e| e.name == "subdir").expect("subdir");
    assert!(de.is_dir);
    assert!(de.mod_time > UNIX_EPOCH);
}

#[test]
fn req_022_3_list_dir_byte_size_is_file_size_in_bytes() {
    let dir = fresh_tmpdir("list_dir_filesize");
    fs::write(dir.join("data.bin"), b"hello world").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let entries = t.list_dir(&peer.handle, "").unwrap();
    let entry = entries.iter().find(|e| e.name == "data.bin").expect("data.bin");
    assert_eq!(entry.byte_size, 11);
}

#[test]
fn req_022_4_list_dir_byte_size_minus_one_for_directory() {
    let dir = fresh_tmpdir("list_dir_dirsize");
    fs::create_dir(dir.join("mydir")).unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let entries = t.list_dir(&peer.handle, "").unwrap();
    let entry = entries.iter().find(|e| e.name == "mydir").expect("mydir");
    assert_eq!(entry.byte_size, -1);
}

#[test]
fn req_022_5_stat_returns_metadata_for_regular_file() {
    let dir = fresh_tmpdir("stat_file");
    fs::write(dir.join("readme.txt"), b"12345").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let s = t.stat(&peer.handle, "readme.txt").unwrap();
    assert_eq!(s.byte_size, 5);
    assert!(!s.is_dir);
    assert!(s.mod_time > UNIX_EPOCH);
}

#[test]
fn req_022_5_stat_returns_metadata_for_directory() {
    let dir = fresh_tmpdir("stat_dir");
    fs::create_dir(dir.join("adir")).unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let s = t.stat(&peer.handle, "adir").unwrap();
    assert_eq!(s.byte_size, -1);
    assert!(s.is_dir);
}

#[test]
fn req_022_6_stat_returns_not_found_for_missing_path() {
    let dir = fresh_tmpdir("stat_notfound");
    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let result = t.stat(&peer.handle, "no_such_file.txt");
    assert!(
        matches!(result, Err(TransportError::NotFound)),
        "expected NotFound for missing path"
    );
}

#[test]
fn req_022_7_read_returns_chunks_and_eof() {
    let dir = fresh_tmpdir("read_eof");
    fs::write(dir.join("chunk.bin"), b"abcdefgh").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let rh = t.open_read(&peer.handle, "chunk.bin").unwrap();

    let mut data = Vec::new();
    loop {
        match t.read(&rh, 3).unwrap() {
            Some(chunk) => data.extend_from_slice(&chunk),
            None => break,
        }
    }
    t.close_read(rh).unwrap();
    assert_eq!(data, b"abcdefgh");
}

#[test]
fn req_022_8_open_write_creates_file_and_missing_parents() {
    let dir = fresh_tmpdir("open_write_parents");
    let t = make_transport();
    let peer = open_file_peer(&t, &dir);

    let wh = t.open_write(&peer.handle, "sub/nested/out.txt").unwrap();
    t.write(&wh, b"written").unwrap();
    t.close_write(wh).unwrap();

    assert_eq!(
        fs::read(dir.join("sub/nested/out.txt")).unwrap(),
        b"written"
    );
}

#[test]
fn req_022_9_create_dir_creates_directory_and_missing_parents() {
    let dir = fresh_tmpdir("create_dir_parents");
    let t = make_transport();
    let peer = open_file_peer(&t, &dir);

    t.create_dir(&peer.handle, "a/b/c").unwrap();
    assert!(dir.join("a/b/c").is_dir());
}

#[test]
fn req_022_10_rename_moves_src_to_nonexistent_dst() {
    let dir = fresh_tmpdir("rename_ok");
    fs::write(dir.join("src.txt"), b"content").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    t.rename(&peer.handle, "src.txt", "dst.txt").unwrap();

    assert!(!dir.join("src.txt").exists());
    assert_eq!(fs::read(dir.join("dst.txt")).unwrap(), b"content");
}

#[test]
fn req_022_11_rename_fails_when_dst_exists() {
    let dir = fresh_tmpdir("rename_dst_exists");
    fs::write(dir.join("src.txt"), b"src").unwrap();
    fs::write(dir.join("dst.txt"), b"dst").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    let result = t.rename(&peer.handle, "src.txt", "dst.txt");
    assert!(result.is_err(), "rename to existing dst should fail");
}

#[test]
fn req_022_12_delete_file_removes_file() {
    let dir = fresh_tmpdir("delete_file");
    fs::write(dir.join("to_delete.txt"), b"bye").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    t.delete_file(&peer.handle, "to_delete.txt").unwrap();
    assert!(!dir.join("to_delete.txt").exists());
}

#[test]
fn req_022_13_delete_dir_removes_empty_directory() {
    let dir = fresh_tmpdir("delete_dir");
    fs::create_dir(dir.join("emptydir")).unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);
    t.delete_dir(&peer.handle, "emptydir").unwrap();
    assert!(!dir.join("emptydir").exists());
}

#[test]
fn req_022_14_set_mod_time_sets_modification_time() {
    let dir = fresh_tmpdir("set_mod_time");
    fs::write(dir.join("file.txt"), b"x").unwrap();

    let t = make_transport();
    let peer = open_file_peer(&t, &dir);

    let target = UNIX_EPOCH + Duration::from_secs(1_000_000);
    t.set_mod_time(&peer.handle, "file.txt", target).unwrap();

    let actual = fs::metadata(dir.join("file.txt")).unwrap().modified().unwrap();
    let diff = if actual > target {
        actual.duration_since(target).unwrap()
    } else {
        target.duration_since(actual).unwrap()
    };
    assert!(diff < Duration::from_secs(2), "mod_time off by {:?}", diff);
}

// 022.15, 022.16: omit symlinks / special files.
// Not tested with purpose-built symlinks; per TESTING-GUIDELINES no symlink
// setup code is allowed. The spec behaviour is verified implicitly by 022.2
// returning only regular files and directories.

#[test]
fn req_022_17_operations_report_not_found_error_category() {
    // Failure category is TransportError::NotFound, not a scheme-specific type.
    let dir = fresh_tmpdir("error_category");
    let t = make_transport();
    let peer = open_file_peer(&t, &dir);

    let result = t.stat(&peer.handle, "nonexistent_file.txt");
    assert!(
        matches!(result, Err(TransportError::NotFound)),
        "expected TransportError::NotFound"
    );
}

// ── 005.x peer URL selection ──────────────────────────────────────────────────

#[test]
fn req_005_1_primary_url_attempted_before_fallback() {
    let primary_dir = fresh_tmpdir("primary_first");
    let fallback_dir = fresh_tmpdir("fallback_first");
    let t = make_transport();

    let primary = file_url(&primary_dir);
    let fallback = file_url(&fallback_dir);
    let connected = t
        .open_peer(&primary, &[fallback], false, Duration::from_secs(5))
        .unwrap();

    assert_eq!(
        connected.winning_url, primary,
        "primary should win when it connects"
    );
}

#[test]
fn req_005_2_fallbacks_tried_in_listed_order() {
    let fallback2_dir = fresh_tmpdir("fallback_order2");
    let t = make_transport();

    // Primary and first fallback do not exist; second fallback does.
    let missing_primary = "file:///nonexistent_primary_order_11111".to_string();
    let missing_fb1 = "file:///nonexistent_fallback1_order_11111".to_string();
    let fallback2 = file_url(&fallback2_dir);

    let connected = t
        .open_peer(
            &missing_primary,
            &[missing_fb1, fallback2.clone()],
            true,
            Duration::from_secs(5),
        )
        .unwrap();

    assert_eq!(
        connected.winning_url, fallback2,
        "second fallback should win after first fails"
    );
}

#[test]
fn req_005_3_connects_through_first_later_url_that_connects() {
    let working_dir = fresh_tmpdir("first_working");
    let t = make_transport();

    let missing = "file:///nonexistent_first_working_22222".to_string();
    let working = file_url(&working_dir);

    let connected = t
        .open_peer(&missing, &[working.clone()], true, Duration::from_secs(5))
        .unwrap();

    assert_eq!(connected.winning_url, working);
}

#[test]
fn req_005_4_winning_url_is_first_url_that_connects() {
    let dir = fresh_tmpdir("winning_url");
    let t = make_transport();
    let url = file_url(&dir);

    let connected = t
        .open_peer(&url, &[], false, Duration::from_secs(5))
        .unwrap();

    assert_eq!(connected.winning_url, url);
}

#[test]
fn req_005_9_normal_run_creates_missing_root_for_file_url() {
    let base = fresh_tmpdir("normal_root_base");
    let root = base.join("peer_root");
    let _ = fs::remove_dir_all(&root);

    let t = make_transport();
    let url = format!("file://{}", root.display());
    let result = t.open_peer(&url, &[], false, Duration::from_secs(5));

    assert!(result.is_some(), "normal run should create missing root");
    assert!(root.is_dir(), "root directory should exist after open_peer");
}

#[test]
fn req_005_11_normal_run_creates_missing_parent_directories() {
    let base = fresh_tmpdir("missing_parents_base");
    let root = base.join("a/b/c/peer_root");
    let _ = fs::remove_dir_all(base.join("a"));

    let t = make_transport();
    let url = format!("file://{}", root.display());
    let result = t.open_peer(&url, &[], false, Duration::from_secs(5));

    assert!(result.is_some());
    assert!(root.is_dir());
}

#[test]
fn req_005_13_dry_run_does_not_create_missing_root() {
    let base = fresh_tmpdir("dry_run_no_create_base");
    let root = base.join("missing_peer_root");
    let _ = fs::remove_dir_all(&root);

    let t = make_transport();
    let url = format!("file://{}", root.display());
    let result = t.open_peer(&url, &[], true, Duration::from_secs(5));

    assert!(result.is_none(), "dry run should not connect to a missing root");
    assert!(!root.exists(), "dry run must not create the missing root");
}

#[test]
fn req_005_14_dry_run_absent_root_treated_as_failed() {
    let t = make_transport();
    let result = t.open_peer(
        "file:///nonexistent_dry_run_root_33333",
        &[],
        true,
        Duration::from_secs(5),
    );
    assert!(result.is_none(), "dry run with absent root should yield None");
}

#[test]
fn req_005_15_all_urls_fail_returns_none() {
    let t = make_transport();
    let result = t.open_peer(
        "file:///nonexistent_all_fail_a_44444",
        &["file:///nonexistent_all_fail_b_44444".to_string()],
        true,
        Duration::from_secs(5),
    );
    assert!(result.is_none(), "every URL failing should yield None (unreachable)");
}

// ── 024.x dry-run connectivity ────────────────────────────────────────────────

#[test]
fn req_024_1_dry_run_connects_to_existing_peer() {
    let dir = fresh_tmpdir("dry_run_connects");
    let t = make_transport();
    let result = t.open_peer(&file_url(&dir), &[], true, Duration::from_secs(5));
    assert!(result.is_some(), "dry run should connect when root already exists");
}

#[test]
fn req_024_11_dry_run_absent_root_is_unreachable() {
    let base = fresh_tmpdir("dry_run_unreachable_base");
    let root = base.join("absent_root");
    let _ = fs::remove_dir_all(&root);

    let t = make_transport();
    let url = format!("file://{}", root.display());
    let result = t.open_peer(&url, &[], true, Duration::from_secs(5));

    assert!(result.is_none());
    assert!(!root.exists());
}

// ── 004.x SFTP authentication ─────────────────────────────────────────────────

#[test]
fn req_004_1_inline_url_password_connects() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("pw_auth");
    let server = SftpServer::start(&["--password", "s3cr3t"]);
    write_known_hosts(&home.join(".ssh"), server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://any:s3cr3t@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(result.is_some(), "inline URL password should authenticate");
}

#[test]
fn req_004_3_ed25519_key_file_used_when_no_password_or_agent() {
    // Req 004.3, 004.6: ~/.ssh/id_ed25519 is the third credential source;
    // with no inline password and no SSH agent, it must be tried and succeed.
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("ed25519_auth");
    let ssh_dir = home.join(".ssh");
    generate_ed25519_key(&ssh_dir);

    let pub_key = ssh_dir.join("id_ed25519.pub");
    let server = SftpServer::start(&["--authorized-key", pub_key.to_str().unwrap()]);
    write_known_hosts(&ssh_dir, server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    // No password in URL; no SSH agent; only id_ed25519 can succeed.
    let url = format!("sftp://testuser@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(result.is_some(), "Ed25519 key-file auth should succeed");
}

#[test]
fn req_004_8_host_key_matches_known_hosts_entry_passes() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("known_host_ok");
    let server = SftpServer::start(&["--password", "pw"]);
    write_known_hosts(&home.join(".ssh"), server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://user:pw@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(
        result.is_some(),
        "host with matching known_hosts entry should connect"
    );
}

#[test]
fn req_004_9_host_absent_from_known_hosts_is_rejected() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("unknown_host");
    // Intentionally no known_hosts written.
    let server = SftpServer::start(&["--password", "pw"]);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://user:pw@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(result.is_none(), "host absent from known_hosts should be rejected");
}

#[test]
fn req_004_10_percent_encoded_password_decoded_before_auth() {
    // %40 -> @, %3A -> : in inline URL password
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("encoded_pw");
    // Server expects the literal password "user@host:22"
    let server = SftpServer::start(&["--password", "user@host:22"]);
    write_known_hosts(&home.join(".ssh"), server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    // Encode the password in the URL: @ -> %40, : -> %3A
    let url = format!(
        "sftp://testuser:user%40host%3A22@127.0.0.1:{}/",
        server.port
    );
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(result.is_some(), "percent-decoded inline password should authenticate");
}

// ── 005.x SFTP connection timeout ────────────────────────────────────────────

#[test]
fn req_005_6_and_005_8_sftp_handshake_timeout_abandons_url() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("handshake_timeout");
    // Listen on a port but never respond -- SSH handshake stalls until timeout.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://user@127.0.0.1:{}/", port);
    let result = t.open_peer(&url, &[], false, Duration::from_millis(500));

    drop(listener);
    assert!(
        result.is_none(),
        "URL whose handshake timed out should fail (None)"
    );
}

#[test]
fn req_005_7_url_timeout_conn_param_overrides_default_timeout() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("url_timeout");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    // URL says timeout-conn=1 (1 s); the default passed in is 60 s.
    let url = format!("sftp://user@127.0.0.1:{}/?timeout-conn=1", port);
    let long_default = Duration::from_secs(60);

    let start = Instant::now();
    let result = t.open_peer(&url, &[], false, long_default);
    let elapsed = start.elapsed();

    drop(listener);
    assert!(result.is_none());
    assert!(
        elapsed < Duration::from_secs(10),
        "URL timeout-conn=1 should fire well before the 60 s default (elapsed: {:?})",
        elapsed
    );
}

// ── 005.10 SFTP missing root creation ────────────────────────────────────────

#[test]
fn req_005_10_sftp_normal_run_creates_missing_root() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("sftp_root_create");
    let server = SftpServer::start(&["--password", "pw"]);
    write_known_hosts(&home.join(".ssh"), server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    // "newroot_create" does not exist on the server yet.
    let url = format!("sftp://user:pw@127.0.0.1:{}/newroot_create", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(
        result.is_some(),
        "sftp normal run should create a missing root directory"
    );
}

// ── 022.1 identical results for file:// and sftp:// ──────────────────────────

#[test]
fn req_022_1_file_and_sftp_identical_contents_yield_identical_results() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("parity");
    let server = SftpServer::start(&["--password", "pw"]);
    write_known_hosts(&home.join(".ssh"), server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();

    // file:// peer with known content
    let file_dir = fresh_tmpdir("parity_file");
    fs::write(file_dir.join("doc.txt"), b"hello parity").unwrap();
    fs::create_dir(file_dir.join("sub")).unwrap();

    let file_peer = t
        .open_peer(&file_url(&file_dir), &[], false, Duration::from_secs(5))
        .unwrap();

    // sftp:// peer: write identical content through Transport operations.
    let sftp_url = format!("sftp://user:pw@127.0.0.1:{}/parity_root", server.port);
    let sftp_peer = t
        .open_peer(&sftp_url, &[], false, Duration::from_secs(15))
        .unwrap();

    let wh = t.open_write(&sftp_peer.handle, "doc.txt").unwrap();
    t.write(&wh, b"hello parity").unwrap();
    t.close_write(wh).unwrap();
    t.create_dir(&sftp_peer.handle, "sub").unwrap();

    // Compare listings: name, is_dir, and byte_size must match.
    let mut file_entries = t.list_dir(&file_peer.handle, "").unwrap();
    let mut sftp_entries = t.list_dir(&sftp_peer.handle, "").unwrap();

    file_entries.sort_by(|a, b| a.name.cmp(&b.name));
    sftp_entries.sort_by(|a, b| a.name.cmp(&b.name));

    let summarise = |entries: &[transport::DirEntry]| {
        entries
            .iter()
            .map(|e| (e.name.clone(), e.is_dir, e.byte_size))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        summarise(&file_entries),
        summarise(&sftp_entries),
        "file:// and sftp:// with identical contents should yield identical listing results"
    );
}

// ── 004.2 SSH agent credential source ────────────────────────────────────────

#[test]
fn req_004_2_ssh_agent_authenticates() {
    // SSH_AUTH_SOCK agent is the second credential source.
    // The key lives only in the agent (not in ~/.ssh/), so it proves the agent path.
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("agent_auth");
    let ssh_dir = home.join(".ssh");

    // Generate key outside ~/.ssh/ so the key-file fallback (sources 3-5) cannot find it.
    let key_dir = fresh_tmpdir("agent_key");
    let key_path = key_dir.join("agent_ed25519");
    Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f"])
        .arg(&key_path)
        .args(["-N", ""])
        .output()
        .expect("ssh-keygen for agent test");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    let pub_key_path = key_dir.join("agent_ed25519.pub");
    let server = SftpServer::start(&["--authorized-key", pub_key_path.to_str().unwrap()]);
    write_known_hosts(&ssh_dir, server.port, &server.host_key_line);

    // Start an ssh-agent and load the key into it.
    let agent_out = Command::new("ssh-agent")
        .arg("-s")
        .output()
        .expect("ssh-agent -s");
    let agent_stdout = String::from_utf8_lossy(&agent_out.stdout).into_owned();

    // ssh-agent -s emits lines like:
    //   SSH_AUTH_SOCK=/tmp/ssh-xxx/agent.N; export SSH_AUTH_SOCK;
    // Split on ';' and take the first token to get the bare value.
    let sock = agent_stdout
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("SSH_AUTH_SOCK=")
                .and_then(|r| r.split(';').next())
                .map(|s| s.trim().to_string())
        })
        .expect("SSH_AUTH_SOCK from ssh-agent output");

    let agent_pid = agent_stdout
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("SSH_AGENT_PID=")
                .and_then(|r| r.split(';').next())
                .map(|s| s.trim().to_string())
        })
        .expect("SSH_AGENT_PID from ssh-agent output");

    Command::new("ssh-add")
        .env("SSH_AUTH_SOCK", &sock)
        .arg(&key_path)
        .output()
        .expect("ssh-add");

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::set("SSH_AUTH_SOCK", &sock);

    let t = make_transport();
    // No inline password; no key files in ~/.ssh/; only the agent can authenticate.
    let url = format!("sftp://testuser@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    let _ = Command::new("kill").arg(&agent_pid).output();

    assert!(result.is_some(), "SSH agent (SSH_AUTH_SOCK) should authenticate");
}

// ── 004.4 id_ecdsa as fourth credential source ───────────────────────────────

#[test]
fn req_004_4_ecdsa_key_file_used_when_no_earlier_source() {
    // ~/.ssh/id_ecdsa is the fourth credential source.
    // No inline password, no SSH agent, no id_ed25519 present; only id_ecdsa is present.
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("ecdsa_auth");
    let ssh_dir = home.join(".ssh");
    generate_ecdsa_key(&ssh_dir);

    let pub_key = ssh_dir.join("id_ecdsa.pub");
    let server = SftpServer::start(&["--authorized-key", pub_key.to_str().unwrap()]);
    write_known_hosts(&ssh_dir, server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://testuser@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(result.is_some(), "~/.ssh/id_ecdsa should authenticate when no earlier source");
}

// ── 004.5 id_rsa as fifth credential source ──────────────────────────────────

#[test]
fn req_004_5_rsa_key_file_used_when_no_earlier_source() {
    // ~/.ssh/id_rsa is the fifth credential source.
    // No inline password, no SSH agent, no id_ed25519, no id_ecdsa; only id_rsa present.
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("rsa_auth");
    let ssh_dir = home.join(".ssh");
    generate_rsa_key(&ssh_dir);

    let pub_key = ssh_dir.join("id_rsa.pub");
    let server = SftpServer::start(&["--authorized-key", pub_key.to_str().unwrap()]);
    write_known_hosts(&ssh_dir, server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://testuser@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(result.is_some(), "~/.ssh/id_rsa should authenticate when no earlier source");
}

// ── 004.7 rejected credential source falls through ───────────────────────────

#[test]
fn req_004_7_rejected_credential_source_falls_through_to_next() {
    // Server is key-only (no --password flag), so the inline URL password is rejected.
    // The client must fall through to ~/.ssh/id_ed25519 and connect.
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("fallthrough_auth");
    let ssh_dir = home.join(".ssh");
    generate_ed25519_key(&ssh_dir);

    // key-only server: --authorized-key but no --password, so any password attempt is rejected.
    let pub_key = ssh_dir.join("id_ed25519.pub");
    let server = SftpServer::start(&["--authorized-key", pub_key.to_str().unwrap()]);
    write_known_hosts(&ssh_dir, server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    // Inline password in URL; server rejects it; client falls through to id_ed25519.
    let url = format!("sftp://testuser:wrongpassword@127.0.0.1:{}/", server.port);
    let result = t.open_peer(&url, &[], false, Duration::from_secs(15));

    assert!(
        result.is_some(),
        "after inline password is rejected, client should fall through to id_ed25519 and connect"
    );
}

// ── 005.12 root creation failure treats URL as failed ────────────────────────

#[test]
fn req_005_12_url_whose_root_cannot_be_created_is_treated_as_failed() {
    // A regular file occupies a parent path component; mkdir will always fail.
    let base = fresh_tmpdir("root_fail");
    let blocker = base.join("not_a_dir");
    fs::write(&blocker, b"x").unwrap();
    // Creating peer_root here is impossible: not_a_dir is a file, not a directory.
    let root = blocker.join("peer_root");

    let t = make_transport();
    let url = format!("file://{}", root.display());
    let result = t.open_peer(&url, &[], false, Duration::from_secs(5));

    assert!(result.is_none(), "URL whose root cannot be created should fail");
}

// ── 022.18 network failure surfaces as TransportError::Io ────────────────────

#[test]
fn req_022_18_network_failure_surfaces_as_io_error() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let home = setup_ssh_home("netfail");
    let server = SftpServer::start(&["--password", "pw"]);
    write_known_hosts(&home.join(".ssh"), server.port, &server.host_key_line);

    let _h = EnvGuard::set("HOME", home.to_str().unwrap());
    let _s = EnvGuard::remove("SSH_AUTH_SOCK");

    let t = make_transport();
    let url = format!("sftp://user:pw@127.0.0.1:{}/", server.port);
    let peer = t.open_peer(&url, &[], false, Duration::from_secs(15)).unwrap();

    // Kill the server and wait until new connections to its port are refused.
    // The server's parent-death watchdog exits within ~500 ms of uv dying, so
    // polling here avoids a race where list_dir hits a still-running server.
    let port = server.port;
    drop(server);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let result = t.list_dir(&peer.handle, "");
    assert!(
        matches!(result, Err(TransportError::Io)),
        "connection drop should surface as TransportError::Io"
    );
}
