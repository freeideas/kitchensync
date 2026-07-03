use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peertransportsurface::{PeerReadChunk, PeerTransportError};
use sftptransport::{new, SftpConnectionRequest, SftpTransport};

const PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
QyNTUxOQAAACAhuMO8by9HNFpXlbtItwY6N3tl18y6dmuiqcvhl8dzRgAAAJivdUh1r3VI\n\
dQAAAAtzc2gtZWQyNTUxOQAAACAhuMO8by9HNFpXlbtItwY6N3tl18y6dmuiqcvhl8dzRg\n\
AAAEALRF/BTksyYA5wJjMqgnjDh9my9NN9Ecr91X3UGbpB7yG4w7xvL0c0WleVu0i3Bjo3\n\
e2XXzLp2a6Kpy+GXx3NGAAAAEGtpdGNoZW5zeW5jLXBsYW4BAgMEBQ==\n\
-----END OPENSSH PRIVATE KEY-----\n";

const PUBLIC_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICG4w7xvL0c0WleVu0i3Bjo3e2XXzLp2a6Kpy+GXx3NG kitchensync-plan\n";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn stop_child_bounded(child: &mut Child) {
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct TestServer {
    child: Child,
    port: u16,
    stderr_thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn stop(&mut self) {
        stop_child_bounded(&mut self.child);
        let _ = self.stderr_thread.take();
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> io::Result<Self> {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "kitchensync-sftptransport-tests-{}-{}-{}",
            std::process::id(),
            id,
            name
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct EnvGuard {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn set(home: &Path) -> Self {
        let names = ["HOME", "USERPROFILE", "SSH_AUTH_SOCK"];
        let saved = names
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();
        // Environment mutation is serialized by ENV_LOCK for these tests.
        unsafe {
            std::env::set_var("HOME", home);
            std::env::set_var("USERPROFILE", home);
            std::env::remove_var("SSH_AUTH_SOCK");
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

fn workspace_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("current directory");
    loop {
        if dir.join("extart/ephemeral-sftp-server.py").is_file() {
            return dir;
        }
        assert!(dir.pop(), "could not find workspace root");
    }
}

fn bundled_uv(workspace: &Path) -> PathBuf {
    if cfg!(windows) {
        workspace.join("aisf/bin/uv.exe")
    } else if cfg!(target_os = "macos") {
        workspace.join("aisf/bin/uv.mac")
    } else {
        workspace.join("aisf/bin/uv.linux")
    }
}

fn read_line_with_timeout<R>(reader: R, timeout: Duration) -> io::Result<String>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    rx.recv_timeout(timeout)
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "timed out reading server output"))?
}

fn drain_stderr_and_report_host_key<R>(
    reader: R,
    timeout: Duration,
) -> io::Result<(String, thread::JoinHandle<()>)>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut sent = false;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if !sent {
                        if let Some(rest) = line.strip_prefix("host key: ") {
                            sent = true;
                            let _ = tx.send(Ok(rest.trim().to_string()));
                        }
                    }
                }
                Err(error) => {
                    if !sent {
                        let _ = tx.send(Err(error));
                    }
                    return;
                }
            }
        }
        if !sent {
            let _ = tx.send(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "server did not print a host key line",
            )));
        }
    });
    let host_key = rx
        .recv_timeout(timeout)
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "timed out reading host key"))??;
    Ok((host_key, thread))
}

fn start_server(home: &Path, args: &[&str]) -> TestServer {
    let workspace = workspace_root();
    let mut child = Command::new(bundled_uv(&workspace))
        .arg("run")
        .arg("--script")
        .arg(workspace.join("extart/ephemeral-sftp-server.py"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start ephemeral SFTP server");

    let stdout = child.stdout.take().expect("server stdout");
    let port_line = match read_line_with_timeout(stdout, Duration::from_secs(10)) {
        Ok(line) => line,
        Err(error) => {
            stop_child_bounded(&mut child);
            panic!("server did not report a port: {error}");
        }
    };
    let port = port_line.trim().parse().expect("server port");

    let stderr = child.stderr.take().expect("server stderr");
    let (host_key, stderr_thread) =
        match drain_stderr_and_report_host_key(stderr, Duration::from_secs(10)) {
            Ok(value) => value,
            Err(error) => {
                stop_child_bounded(&mut child);
                panic!("server did not report a host key: {error}");
            }
        };

    write_known_hosts(home, port, &host_key);

    TestServer {
        child,
        port,
        stderr_thread: Some(stderr_thread),
    }
}

fn write_known_hosts(home: &Path, port: u16, host_key: &str) {
    let ssh = home.join(".ssh");
    fs::create_dir_all(&ssh).expect("create .ssh");
    fs::write(
        ssh.join("known_hosts"),
        format!("[127.0.0.1]:{} {}\n", port, host_key),
    )
    .expect("write known_hosts");
}

fn write_ed25519_identity(home: &Path) -> PathBuf {
    let ssh = home.join(".ssh");
    fs::create_dir_all(&ssh).expect("create .ssh");
    let private_path = ssh.join("id_ed25519");
    fs::write(&private_path, PRIVATE_KEY).expect("write private key");
    fs::write(ssh.join("id_ed25519.pub"), PUBLIC_KEY).expect("write public key");
    private_path
}

fn request(port: u16, password: Option<&str>, create_missing_root: bool) -> SftpConnectionRequest {
    request_with_root(port, "/peer/root", password, create_missing_root)
}

fn request_with_root(
    port: u16,
    remote_root_path: &str,
    password: Option<&str>,
    create_missing_root: bool,
) -> SftpConnectionRequest {
    SftpConnectionRequest {
        user: "test-user".to_string(),
        host: "127.0.0.1".to_string(),
        port,
        remote_root_path: remote_root_path.to_string(),
        inline_password: password.map(str::to_string),
        global_timeout_conn_seconds: 30,
        global_timeout_idle_seconds: 30,
        url_timeout_conn_seconds: None,
        url_timeout_idle_seconds: Some(5),
        create_missing_root,
    }
}

#[test]
fn unknown_host_key_is_rejected_before_authentication() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = TempDir::new("unknown-host").unwrap();
    let _env = EnvGuard::set(&temp.path);
    let server = start_server(&temp.path, &["--user", "test-user", "--password", "secret"]);
    fs::write(temp.path.join(".ssh/known_hosts"), "").expect("clear known_hosts");

    let subject = new(peertransportsurface::new());

    let result = subject.connect(request(server.port, Some("secret"), true));

    assert!(
        result.is_err(),
        "a server absent from known_hosts must not be accepted"
    );
}

#[test]
fn url_connection_timeout_override_bounds_the_ssh_handshake() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = TempDir::new("timeout").unwrap();
    let _env = EnvGuard::set(&temp.path);
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local listener");
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let accept_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((_stream, _addr)) => {
                    thread::sleep(Duration::from_secs(4));
                    return;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
    });
    let subject = new(peertransportsurface::new());
    let mut request = request(port, Some("secret"), true);
    request.global_timeout_conn_seconds = 30;
    request.url_timeout_conn_seconds = Some(1);
    let started = Instant::now();

    let result = subject.connect(request);

    assert!(
        result.is_err(),
        "a stalled SSH handshake must fail the candidate"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the URL timeout override must bound the handshake"
    );
    accept_thread.join().unwrap();
}

#[test]
fn password_connection_creates_root_and_supports_root_relative_operations() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = TempDir::new("password-ops").unwrap();
    let _env = EnvGuard::set(&temp.path);
    let server = start_server(&temp.path, &["--user", "test-user", "--password", "secret"]);
    let subject = new(peertransportsurface::new());
    let peer = subject
        .connect(request(server.port, Some("secret"), true))
        .expect("connect with known host and inline password");

    let mut writer = subject
        .open_write(&peer, "alpha/CasePreserved.txt")
        .expect("open writer and create parents");
    subject.write(&mut writer, b"hello ").expect("write first chunk");
    subject
        .write(&mut writer, b"over sftp")
        .expect("write second chunk");
    subject.close_write(writer).expect("close writer");

    let root_entries = subject.list_dir(&peer, "").expect("list root");
    assert!(
        root_entries
            .iter()
            .any(|entry| entry.child_name == "alpha" && entry.is_dir && entry.byte_size == -1),
        "created parent directory must appear at the connected root"
    );

    let alpha_entries = subject.list_dir(&peer, "alpha").expect("list child dir");
    assert_eq!(alpha_entries.len(), 1);
    assert_eq!(alpha_entries[0].child_name, "CasePreserved.txt");
    assert!(!alpha_entries[0].is_dir);
    assert_eq!(alpha_entries[0].byte_size, 15);

    let metadata = subject
        .stat(&peer, "alpha/CasePreserved.txt")
        .expect("stat written file");
    assert!(!metadata.is_dir);
    assert_eq!(metadata.byte_size, 15);

    let mut reader = subject
        .open_read(&peer, "alpha/CasePreserved.txt")
        .expect("open reader");
    let mut bytes = Vec::new();
    loop {
        match subject.read(&mut reader, 5).expect("read chunk") {
            PeerReadChunk::Bytes(chunk) => {
                assert!(chunk.len() <= 5);
                bytes.extend(chunk);
            }
            PeerReadChunk::Eof => break,
        }
    }
    subject.close_read(reader).expect("close reader");
    assert_eq!(bytes, b"hello over sftp");

    let mod_time = UNIX_EPOCH + Duration::from_secs(1_704_110_400);
    subject
        .set_mod_time(&peer, "alpha/CasePreserved.txt", mod_time)
        .expect("set mtime");
    let stored = subject
        .stat(&peer, "alpha/CasePreserved.txt")
        .expect("stat after mtime")
        .mod_time
        .duration_since(UNIX_EPOCH)
        .expect("mtime after epoch")
        .as_secs();
    assert_eq!(stored, 1_704_110_400);

    subject
        .rename(&peer, "alpha/CasePreserved.txt", "alpha/renamed.txt")
        .expect("rename to non-existing destination");
    assert_eq!(
        subject.stat(&peer, "alpha/CasePreserved.txt"),
        Err(PeerTransportError::NotFound)
    );

    let mut renamed = subject.open_read(&peer, "alpha/renamed.txt").unwrap();
    assert_eq!(
        subject.read(&mut renamed, 100).unwrap(),
        PeerReadChunk::Bytes(b"hello over sftp".to_vec())
    );
    subject.close_read(renamed).unwrap();

    subject
        .delete_file(&peer, "alpha/renamed.txt")
        .expect("delete file");
    assert_eq!(
        subject.stat(&peer, "alpha/renamed.txt"),
        Err(PeerTransportError::NotFound)
    );

    subject
        .create_dir(&peer, "empty/child")
        .expect("create directory parents");
    let dir_metadata = subject.stat(&peer, "empty/child").expect("stat dir");
    assert!(dir_metadata.is_dir);
    assert_eq!(dir_metadata.byte_size, -1);
    let dir_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_704_110_401);
    subject
        .set_mod_time(&peer, "empty/child", dir_time)
        .expect("set directory mtime");
    subject
        .delete_dir(&peer, "empty/child")
        .expect("delete empty child");
    subject
        .delete_dir(&peer, "empty")
        .expect("delete empty parent");
    assert_eq!(
        subject.stat(&peer, "empty"),
        Err(PeerTransportError::NotFound)
    );
}

#[test]
fn connected_handles_keep_operations_scoped_to_their_remote_root() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = TempDir::new("root-scope").unwrap();
    let _env = EnvGuard::set(&temp.path);
    let server = start_server(&temp.path, &["--user", "test-user", "--password", "secret"]);
    let subject = new(peertransportsurface::new());
    let left = subject
        .connect(request_with_root(server.port, "/left", Some("secret"), true))
        .unwrap();
    let right = subject
        .connect(request_with_root(server.port, "/right", Some("secret"), true))
        .unwrap();

    let mut left_writer = subject.open_write(&left, "same.txt").unwrap();
    subject.write(&mut left_writer, b"left").unwrap();
    subject.close_write(left_writer).unwrap();
    let mut right_writer = subject.open_write(&right, "same.txt").unwrap();
    subject.write(&mut right_writer, b"right").unwrap();
    subject.close_write(right_writer).unwrap();

    let mut left_reader = subject.open_read(&left, "same.txt").unwrap();
    let mut right_reader = subject.open_read(&right, "same.txt").unwrap();

    assert_eq!(
        subject.read(&mut left_reader, 16).unwrap(),
        PeerReadChunk::Bytes(b"left".to_vec())
    );
    assert_eq!(
        subject.read(&mut right_reader, 16).unwrap(),
        PeerReadChunk::Bytes(b"right".to_vec())
    );
    subject.close_read(left_reader).unwrap();
    subject.close_read(right_reader).unwrap();
}

#[test]
fn connected_operation_reports_io_error_after_sftp_connection_drop() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = TempDir::new("drop").unwrap();
    let _env = EnvGuard::set(&temp.path);
    let mut server = start_server(&temp.path, &["--user", "test-user", "--password", "secret"]);
    let subject = new(peertransportsurface::new());
    let peer = subject
        .connect(request(server.port, Some("secret"), true))
        .unwrap();

    server.stop();
    thread::sleep(Duration::from_millis(800));

    assert_eq!(subject.list_dir(&peer, ""), Err(PeerTransportError::IoError));
}

#[test]
fn key_only_server_accepts_saved_ed25519_identity_without_password_or_agent() {
    let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = TempDir::new("ed25519").unwrap();
    let _env = EnvGuard::set(&temp.path);
    let public_key = write_ed25519_identity(&temp.path).with_extension("pub");
    let public_key_arg = public_key.to_string_lossy().into_owned();
    let server = start_server(
        &temp.path,
        &[
            "--user",
            "test-user",
            "--authorized-key",
            public_key_arg.as_str(),
        ],
    );
    let subject = new(peertransportsurface::new());

    let peer = subject
        .connect(request(server.port, None, true))
        .expect("connect through saved Ed25519 key");

    subject
        .create_dir(&peer, "key-auth-created")
        .expect("SFTP subsystem is usable after key authentication");
    assert!(subject.stat(&peer, "key-auth-created").unwrap().is_dir);
}
