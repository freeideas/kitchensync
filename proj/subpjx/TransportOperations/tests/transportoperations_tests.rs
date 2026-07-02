use ssh2::{CheckResult, KnownHostFileKind, Session};
use std::any::Any;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use transportoperations::{
    new, TransportEntryType, TransportErrorCategory, TransportOperations, TransportPeerHandle,
    TransportReadResult,
};
use transportoperations_localtransportoperations::LocalTransportRoot;

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kitchensync-transportoperations-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct EphemeralSftpServer {
    child: Child,
    port: u16,
    host_key: String,
    stderr_thread: Option<thread::JoinHandle<()>>,
}

impl EphemeralSftpServer {
    fn start() -> Self {
        let root = workspace_root();
        let mut child = Command::new(uv_path(&root))
            .arg("run")
            .arg("--script")
            .arg(root.join("extart").join("ephemeral-sftp-server.py"))
            .arg("--user")
            .arg("alice")
            .arg("--password")
            .arg("secret")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start ephemeral SFTP server");

        let stdout = child.stdout.take().expect("server stdout");
        let (port_sender, port_receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout)
                .read_line(&mut line)
                .map(|_| line)
                .map_err(|error| error.to_string());
            let _ = port_sender.send(result);
        });
        let port_line = port_receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("server printed port in time")
            .expect("read server port");
        let port = port_line.trim().parse().expect("parse server port");

        let stderr = child.stderr.take().expect("server stderr");
        let (host_key_sender, host_key_receiver) = mpsc::channel();
        let stderr_thread = thread::spawn(move || {
            let mut sent_host_key = false;
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    break;
                };
                if !sent_host_key {
                    if let Some(host_key) = line.trim().strip_prefix("host key: ") {
                        let _ = host_key_sender.send(host_key.to_string());
                        sent_host_key = true;
                    }
                }
            }
        });
        let host_key = host_key_receiver
            .recv_timeout(Duration::from_secs(15))
            .expect("server printed host key in time");

        Self {
            child,
            port,
            host_key,
            stderr_thread: Some(stderr_thread),
        }
    }

    fn connect(&self) -> Session {
        let tcp = TcpStream::connect(("127.0.0.1", self.port)).expect("connect to SFTP server");
        tcp.set_read_timeout(Some(Duration::from_secs(10)))
            .expect("set read timeout");
        tcp.set_write_timeout(Some(Duration::from_secs(10)))
            .expect("set write timeout");
        let mut session = Session::new().expect("create SSH session");
        session.set_tcp_stream(tcp);
        session.handshake().expect("SSH handshake");

        let (raw_key, _) = session.host_key().expect("session host key");
        let mut known_hosts = session.known_hosts().expect("known hosts");
        let known_line = format!("[127.0.0.1]:{} {}", self.port, self.host_key);
        known_hosts
            .read_str(&known_line, KnownHostFileKind::OpenSSH)
            .expect("read known hosts line");
        assert!(matches!(
            known_hosts.check_port("127.0.0.1", self.port, raw_key),
            CheckResult::Match
        ));

        session
            .userauth_password("alice", "secret")
            .expect("password authentication");
        assert!(session.authenticated());
        session
    }
}

impl Drop for EphemeralSftpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("subpjx directory")
        .parent()
        .expect("proj directory")
        .parent()
        .expect("workspace directory")
        .to_path_buf()
}

fn uv_path(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("aitc").join("bin").join("uv.exe")
    } else if cfg!(target_os = "macos") {
        root.join("aitc").join("bin").join("uv.mac")
    } else {
        root.join("aitc").join("bin").join("uv.linux")
    }
}

fn subject() -> Arc<dyn TransportOperations> {
    new(
        transportoperations_localtransportoperations::new(),
        transportoperations_sftptransportoperations::new(),
    )
}

fn file_peer(root: &Path) -> TransportPeerHandle {
    TransportPeerHandle::File {
        root: root.to_path_buf(),
        handle: Arc::new(LocalTransportRoot {
            local_peer_root_path: root.to_path_buf(),
        }) as Arc<dyn Any + Send + Sync>,
    }
}

fn sftp_peer(server: &EphemeralSftpServer, root: &str) -> TransportPeerHandle {
    let session = server.connect();
    let sftp = session.sftp().expect("open SFTP subsystem");
    TransportPeerHandle::Sftp {
        root: root.to_string(),
        handle: Arc::new(Mutex::new(sftp)) as Arc<dyn Any + Send + Sync>,
    }
}

#[test]
fn file_peer_lists_only_immediate_regular_files_and_directories() {
    let root = TestRoot::new("list-dir");
    fs::write(root.path().join("alpha.txt"), b"abc").expect("write child file");
    fs::create_dir_all(root.path().join("bravo").join("nested")).expect("create nested dir");
    fs::write(root.path().join("bravo").join("nested.txt"), b"hidden")
        .expect("write nested file");

    let transport = subject();
    let peer = file_peer(root.path());
    let mut entries = transport
        .list_dir(&peer, "")
        .expect("list file peer root through transport trait");
    entries.sort_by(|left, right| left.name.cmp(&right.name));

    assert_eq!(2, entries.len());
    assert_eq!("alpha.txt", entries[0].name);
    assert_eq!(TransportEntryType::File, entries[0].metadata.entry_type);
    assert_eq!(3, entries[0].metadata.byte_size);
    assert!(entries[0].metadata.modification_time <= SystemTime::now());
    assert_eq!("bravo", entries[1].name);
    assert_eq!(
        TransportEntryType::Directory,
        entries[1].metadata.entry_type
    );
    assert_eq!(-1, entries[1].metadata.byte_size);
    assert!(entries[1].metadata.modification_time <= SystemTime::now());
}

#[test]
fn file_peer_stat_reports_regular_file_directory_and_absence_categories() {
    let root = TestRoot::new("stat");
    fs::write(root.path().join("file.txt"), b"content").expect("write file");
    fs::create_dir(root.path().join("directory")).expect("create directory");

    let transport = subject();
    let peer = file_peer(root.path());

    let file = transport.stat(&peer, "file.txt").expect("stat regular file");
    assert_eq!(TransportEntryType::File, file.entry_type);
    assert_eq!(7, file.byte_size);
    assert!(file.modification_time <= SystemTime::now());

    let directory = transport.stat(&peer, "directory").expect("stat directory");
    assert_eq!(TransportEntryType::Directory, directory.entry_type);
    assert_eq!(-1, directory.byte_size);
    assert!(directory.modification_time <= SystemTime::now());

    let missing = transport
        .stat(&peer, "missing.txt")
        .expect_err("missing path is reported as not found");
    assert_eq!(TransportErrorCategory::NotFound, missing.category);
}

#[test]
fn file_peer_streaming_write_creates_parents_and_streaming_read_returns_eof() {
    let root = TestRoot::new("streaming");
    let transport = subject();
    let peer = file_peer(root.path());

    let writer = transport
        .open_write(&peer, "new/parent/file.txt")
        .expect("open write creates target and parents");
    transport.write(&writer, b"hello ").expect("write first chunk");
    transport.write(&writer, b"world").expect("write second chunk");
    transport.close_write(writer).expect("close write handle");

    let reader = transport
        .open_read(&peer, "new/parent/file.txt")
        .expect("open read for written regular file");
    assert_eq!(
        TransportReadResult::Bytes(b"hello".to_vec()),
        transport.read(&reader, 5).expect("read first bytes")
    );
    assert_eq!(
        TransportReadResult::Bytes(b" world".to_vec()),
        transport.read(&reader, 20).expect("read remaining bytes")
    );
    assert_eq!(
        TransportReadResult::Eof,
        transport.read(&reader, 20).expect("read eof")
    );
    transport.close_read(reader).expect("close read handle");
}

#[test]
fn file_peer_mutates_entries_and_rejects_rename_over_existing_destination() {
    let root = TestRoot::new("mutations");
    fs::write(root.path().join("source.txt"), b"move me").expect("write source");
    fs::write(root.path().join("existing.txt"), b"keep me").expect("write existing");

    let transport = subject();
    let peer = file_peer(root.path());

    let existing_destination = transport
        .rename(&peer, "source.txt", "existing.txt")
        .expect_err("rename to existing destination is not required to overwrite");
    assert_eq!(TransportErrorCategory::IoError, existing_destination.category);
    assert_eq!(
        b"keep me",
        fs::read(root.path().join("existing.txt"))
            .expect("existing destination remains readable")
            .as_slice()
    );

    transport
        .rename(&peer, "source.txt", "renamed.txt")
        .expect("rename to missing destination");
    assert_eq!(
        TransportErrorCategory::NotFound,
        transport.stat(&peer, "source.txt").expect_err("source moved").category
    );
    assert_eq!(
        7,
        transport
            .stat(&peer, "renamed.txt")
            .expect("destination exists")
            .byte_size
    );

    let file_time = UNIX_EPOCH + Duration::from_secs(1_700_000_123);
    transport
        .set_mod_time(&peer, "renamed.txt", file_time)
        .expect("set file modification time");
    assert_eq!(
        file_time,
        transport
            .stat(&peer, "renamed.txt")
            .expect("stat renamed file")
            .modification_time
    );

    transport
        .create_dir(&peer, "created/child")
        .expect("create directory and parents");
    let dir_time = UNIX_EPOCH + Duration::from_secs(1_700_000_456);
    transport
        .set_mod_time(&peer, "created/child", dir_time)
        .expect("set directory modification time");
    assert_eq!(
        dir_time,
        transport
            .stat(&peer, "created/child")
            .expect("stat created directory")
            .modification_time
    );

    transport
        .delete_file(&peer, "renamed.txt")
        .expect("delete regular file");
    assert_eq!(
        TransportErrorCategory::NotFound,
        transport
            .stat(&peer, "renamed.txt")
            .expect_err("deleted file is absent")
            .category
    );

    transport
        .delete_dir(&peer, "created/child")
        .expect("delete empty directory");
    assert_eq!(
        TransportErrorCategory::NotFound,
        transport
            .stat(&peer, "created/child")
            .expect_err("deleted directory is absent")
            .category
    );
}

#[test]
fn file_peer_scopes_relative_paths_to_the_connected_root() {
    let root = TestRoot::new("root-scope");
    fs::write(root.path().join("inside.txt"), b"inside").expect("write inside file");
    let outside_path = root
        .path()
        .parent()
        .expect("test root has a parent")
        .join(format!("outside-{}-root-scope.txt", std::process::id()));
    let _ = fs::remove_file(&outside_path);
    fs::write(&outside_path, b"outside").expect("write outside file");

    let transport = subject();
    let peer = file_peer(root.path());

    assert_eq!(
        6,
        transport
            .stat(&peer, "inside.txt")
            .expect("relative path is resolved inside connected root")
            .byte_size
    );

    let escaped_path = format!(
        "../{}",
        outside_path
            .file_name()
            .expect("outside path has a file name")
            .to_string_lossy()
    );
    assert_eq!(
        TransportErrorCategory::NotFound,
        transport
            .stat(&peer, &escaped_path)
            .expect_err("stat cannot escape the connected root")
            .category
    );
    assert_eq!(
        TransportErrorCategory::NotFound,
        transport
            .open_write(&peer, &escaped_path)
            .expect_err("write cannot escape the connected root")
            .category
    );
    assert_eq!(
        b"outside",
        fs::read(&outside_path)
            .expect("outside file is unchanged")
            .as_slice()
    );

    let _ = fs::remove_file(&outside_path);
}

#[test]
fn file_peer_rename_preserves_a_directory_subtree_at_a_missing_destination() {
    let root = TestRoot::new("rename-directory");
    fs::create_dir_all(root.path().join("source").join("child")).expect("create source tree");
    fs::write(root.path().join("source").join("child").join("file.txt"), b"kept")
        .expect("write nested file");

    let transport = subject();
    let peer = file_peer(root.path());

    transport
        .rename(&peer, "source", "moved")
        .expect("rename directory to missing destination");

    assert_eq!(
        TransportErrorCategory::NotFound,
        transport
            .stat(&peer, "source")
            .expect_err("source directory moved away")
            .category
    );
    let nested = transport
        .stat(&peer, "moved/child/file.txt")
        .expect("nested file remains after directory rename");
    assert_eq!(TransportEntryType::File, nested.entry_type);
    assert_eq!(4, nested.byte_size);

    let reader = transport
        .open_read(&peer, "moved/child/file.txt")
        .expect("open nested file after directory rename");
    assert_eq!(
        TransportReadResult::Bytes(b"kept".to_vec()),
        transport
            .read(&reader, 10)
            .expect("read nested file after directory rename")
    );
    transport.close_read(reader).expect("close nested file reader");
}

#[test]
fn sftp_peer_uses_connected_sftp_handle_for_transport_operations() {
    let server = EphemeralSftpServer::start();
    let transport = subject();
    let peer = sftp_peer(&server, "/");

    transport
        .create_dir(&peer, "alpha/bravo")
        .expect("create remote directory and parents through SFTP");
    let writer = transport
        .open_write(&peer, "alpha/bravo/file.txt")
        .expect("open remote write handle");
    transport
        .write(&writer, b"hello over ")
        .expect("write first SFTP chunk");
    transport
        .write(&writer, b"sftp")
        .expect("write second SFTP chunk");
    transport.close_write(writer).expect("close SFTP writer");

    let metadata = transport
        .stat(&peer, "alpha/bravo/file.txt")
        .expect("stat remote file");
    assert_eq!(TransportEntryType::File, metadata.entry_type);
    assert_eq!(15, metadata.byte_size);

    let reader = transport
        .open_read(&peer, "alpha/bravo/file.txt")
        .expect("open remote read handle");
    assert_eq!(
        TransportReadResult::Bytes(b"hello".to_vec()),
        transport.read(&reader, 5).expect("read first remote bytes")
    );
    assert_eq!(
        TransportReadResult::Bytes(b" over sftp".to_vec()),
        transport
            .read(&reader, 50)
            .expect("read remaining remote bytes")
    );
    assert_eq!(
        TransportReadResult::Eof,
        transport.read(&reader, 50).expect("read remote eof")
    );
    transport.close_read(reader).expect("close SFTP reader");

    let mut entries = transport
        .list_dir(&peer, "alpha")
        .expect("list remote directory");
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(1, entries.len());
    assert_eq!("bravo", entries[0].name);
    assert_eq!(
        TransportEntryType::Directory,
        entries[0].metadata.entry_type
    );
    assert_eq!(-1, entries[0].metadata.byte_size);

    transport
        .rename(&peer, "alpha/bravo/file.txt", "alpha/bravo/renamed.txt")
        .expect("rename remote file to missing destination");
    assert_eq!(
        TransportErrorCategory::NotFound,
        transport
            .stat(&peer, "alpha/bravo/file.txt")
            .expect_err("remote source moved")
            .category
    );

    let remote_time = UNIX_EPOCH + Duration::from_secs(1_700_000_789);
    transport
        .set_mod_time(&peer, "alpha/bravo/renamed.txt", remote_time)
        .expect("set remote file modification time");
    assert_eq!(
        remote_time,
        transport
            .stat(&peer, "alpha/bravo/renamed.txt")
            .expect("stat remote file after mtime")
            .modification_time
    );

    transport
        .delete_file(&peer, "alpha/bravo/renamed.txt")
        .expect("delete remote file");
    transport
        .delete_dir(&peer, "alpha/bravo")
        .expect("delete remote empty directory");
}

#[test]
fn missing_paths_have_the_same_error_category_for_file_and_sftp_peers() {
    let root = TestRoot::new("parity");
    let server = EphemeralSftpServer::start();
    let transport = subject();
    let local_peer = file_peer(root.path());
    let remote_peer = sftp_peer(&server, "/");

    let local_error = transport
        .stat(&local_peer, "missing.txt")
        .expect_err("missing local path");
    let remote_error = transport
        .stat(&remote_peer, "missing.txt")
        .expect_err("missing remote path");

    assert_eq!(TransportErrorCategory::NotFound, local_error.category);
    assert_eq!(local_error.category, remote_error.category);
}

#[test]
fn sftp_network_failure_is_reported_as_io_error() {
    let transport = subject();
    let peer = {
        let server = EphemeralSftpServer::start();
        sftp_peer(&server, "/")
    };

    let error = transport
        .stat(&peer, "anything.txt")
        .expect_err("closed SFTP connection is a transport I/O failure");

    assert_eq!(TransportErrorCategory::IoError, error.category);
}
