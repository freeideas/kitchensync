use peerconnections_sftpurlconnection::{
    new, SftpUrlConnection, SftpUrlConnectionCredentialAttemptStatus,
    SftpUrlConnectionCredentialSource, SftpUrlConnectionEndpoint, SftpUrlConnectionFailureReason,
    SftpUrlConnectionHostKeyFailure, SftpUrlConnectionKnownHosts,
    SftpUrlConnectionRemoteRootFailureKind,
    SftpUrlConnectionRequest, SftpUrlConnectionRunMode,
};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct TempHome {
    path: PathBuf,
}

impl TempHome {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kitchensync-sftpurlconnection-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join(".ssh")).expect("create temp .ssh");
        Self { path }
    }

    fn copy_ed25519_identity_from_plan(&self) {
        let source = workspace_root()
            .join("plan")
            .join("experiments")
            .join("sftp-client")
            .join("id_ed25519");
        let target = self.path.join(".ssh").join("id_ed25519");
        fs::copy(&source, &target).expect("copy Ed25519 identity");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                .expect("set private-key permissions");
        }
    }
}

impl Drop for TempHome {
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
    fn start(args: &[String]) -> Self {
        let root = workspace_root();
        let mut child = Command::new(uv_path(&root))
            .arg("run")
            .arg("--script")
            .arg(root.join("extart").join("ephemeral-sftp-server.py"))
            .args(args)
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

    fn known_hosts_contents(&self) -> String {
        format!("[127.0.0.1]:{} {}", self.port, self.host_key)
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

struct SilentSshPeer {
    port: u16,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl SilentSshPeer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind silent peer");
        let port = listener.local_addr().expect("silent peer address").port();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            if let Ok((_stream, _addr)) = listener.accept() {
                let _ = stop_receiver.recv_timeout(Duration::from_secs(10));
            }
        });
        Self {
            port,
            stop: Some(stop_sender),
            thread: Some(thread),
        }
    }
}

impl Drop for SilentSshPeer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn missing_url_timeout_uses_global_timeout_to_bound_the_handshake() {
    let peer = SilentSshPeer::start();
    let home = TempHome::new("global-timeout");
    let started = Instant::now();

    let failure = new()
        .establish_sftp_url(request(
            peer.port,
            "/timeout",
            home.path.clone(),
            SftpUrlConnectionKnownHosts::Contents(String::new()),
            None,
            None,
            1,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect_err("silent SSH peer must time out during handshake");

    assert_eq!(failure.effective_timeout_conn_seconds, 1);
    assert_eq!(failure.reason, SftpUrlConnectionFailureReason::HandshakeTimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "handshake was not bounded by the global timeout"
    );
}

#[test]
fn url_timeout_overrides_global_timeout_to_bound_the_handshake() {
    let peer = SilentSshPeer::start();
    let home = TempHome::new("url-timeout");
    let started = Instant::now();

    let failure = new()
        .establish_sftp_url(request(
            peer.port,
            "/timeout",
            home.path.clone(),
            SftpUrlConnectionKnownHosts::Contents(String::new()),
            None,
            Some(1),
            30,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect_err("silent SSH peer must time out during handshake");

    assert_eq!(failure.effective_timeout_conn_seconds, 1);
    assert_eq!(failure.reason, SftpUrlConnectionFailureReason::HandshakeTimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "handshake was not bounded by the URL timeout"
    );
}

#[test]
fn normal_run_creates_missing_remote_root_and_parents_before_accepting_url() {
    let server = password_server("secret");
    let home = TempHome::new("remote-root-created");
    let subject = new();

    let established = subject
        .establish_sftp_url(request(
            server.port,
            "/alpha/beta/gamma",
            home.path.clone(),
            SftpUrlConnectionKnownHosts::Contents(server.known_hosts_contents()),
            Some("secret"),
            None,
            9,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect("normal mode should create missing remote root");

    assert_eq!(established.effective_timeout_conn_seconds, 9);
    assert_eq!(
        established.connection.authenticated_with,
        SftpUrlConnectionCredentialSource::InlinePassword
    );

    subject
        .establish_sftp_url(request(
            server.port,
            "/alpha/beta/gamma",
            home.path,
            SftpUrlConnectionKnownHosts::Contents(server.known_hosts_contents()),
            Some("secret"),
            None,
            9,
            SftpUrlConnectionRunMode::DryRun,
        ))
        .expect("dry-run should accept the remote root created by normal mode");
}

#[test]
fn normal_run_treats_remote_root_creation_failure_as_url_failure() {
    let server = password_server("secret");
    let home = TempHome::new("remote-root-fails");

    let failure = new()
        .establish_sftp_url(request(
            server.port,
            "cannot-create\0remote-root",
            home.path,
            SftpUrlConnectionKnownHosts::Contents(server.known_hosts_contents()),
            Some("secret"),
            None,
            9,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect_err("remote path rejected by SFTP should fail this URL");

    assert!(matches!(
        failure.reason,
        SftpUrlConnectionFailureReason::RemoteRootPreparationFailed(failure)
            if failure.kind == SftpUrlConnectionRemoteRootFailureKind::CreationFailed
    ));
}

#[test]
fn untrusted_host_key_rejects_the_sftp_url() {
    let server = password_server("secret");
    let home = TempHome::new("untrusted-host-key");

    let failure = new()
        .establish_sftp_url(request(
            server.port,
            "/host-key",
            home.path,
            SftpUrlConnectionKnownHosts::Contents(String::new()),
            Some("secret"),
            None,
            9,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect_err("missing known-hosts entry should reject the URL");

    assert_eq!(
        failure.reason,
        SftpUrlConnectionFailureReason::HostKeyUntrusted(
            SftpUrlConnectionHostKeyFailure::EntryMissing
        )
    );
}

#[test]
fn authentication_attempts_follow_the_required_fallback_order() {
    let server = password_server("secret");
    let home = TempHome::new("auth-order");

    let failure = new()
        .establish_sftp_url(request(
            server.port,
            "/auth-order",
            home.path,
            SftpUrlConnectionKnownHosts::Contents(server.known_hosts_contents()),
            Some("wrong"),
            None,
            9,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect_err("all credential sources should fail");

    let SftpUrlConnectionFailureReason::AuthenticationExhausted { attempts } = failure.reason
    else {
        panic!("expected authentication exhaustion");
    };

    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt.source)
            .collect::<Vec<_>>(),
        vec![
            SftpUrlConnectionCredentialSource::InlinePassword,
            SftpUrlConnectionCredentialSource::SshAgent,
            SftpUrlConnectionCredentialSource::IdentityFileEd25519,
            SftpUrlConnectionCredentialSource::IdentityFileEcdsa,
            SftpUrlConnectionCredentialSource::IdentityFileRsa,
        ]
    );
    assert!(matches!(
        attempts[0].status,
        SftpUrlConnectionCredentialAttemptStatus::Rejected { .. }
    ));
    assert_eq!(
        attempts[1].status,
        SftpUrlConnectionCredentialAttemptStatus::Absent
    );
    assert_eq!(
        attempts[2].status,
        SftpUrlConnectionCredentialAttemptStatus::Absent
    );
    assert_eq!(
        attempts[3].status,
        SftpUrlConnectionCredentialAttemptStatus::Absent
    );
    assert_eq!(
        attempts[4].status,
        SftpUrlConnectionCredentialAttemptStatus::Absent
    );
}

#[test]
fn ed25519_identity_succeeds_after_inline_password_rejection_and_absent_agent() {
    let home = TempHome::new("ed25519-fallback");
    home.copy_ed25519_identity_from_plan();
    let public_key = workspace_root()
        .join("plan")
        .join("experiments")
        .join("sftp-client")
        .join("id_ed25519.pub");
    let server = EphemeralSftpServer::start(&[
        "--user".to_string(),
        "alice".to_string(),
        "--authorized-key".to_string(),
        public_key.to_string_lossy().to_string(),
    ]);

    let established = new()
        .establish_sftp_url(request(
            server.port,
            "/key-auth",
            home.path,
            SftpUrlConnectionKnownHosts::Contents(server.known_hosts_contents()),
            Some("wrong"),
            None,
            9,
            SftpUrlConnectionRunMode::Normal,
        ))
        .expect("Ed25519 identity should be used after earlier sources fail");

    assert_eq!(
        established.connection.authenticated_with,
        SftpUrlConnectionCredentialSource::IdentityFileEd25519
    );
}

fn password_server(password: &str) -> EphemeralSftpServer {
    EphemeralSftpServer::start(&[
        "--user".to_string(),
        "alice".to_string(),
        "--password".to_string(),
        password.to_string(),
    ])
}

fn request(
    port: u16,
    remote_peer_root_path: &str,
    home_directory: PathBuf,
    known_hosts: SftpUrlConnectionKnownHosts,
    inline_password: Option<&str>,
    url_timeout_conn_seconds: Option<u32>,
    global_timeout_conn_seconds: u32,
    run_mode: SftpUrlConnectionRunMode,
) -> SftpUrlConnectionRequest {
    SftpUrlConnectionRequest {
        endpoint: SftpUrlConnectionEndpoint {
            host: "127.0.0.1".to_string(),
            port,
            username: "alice".to_string(),
        },
        remote_peer_root_path: remote_peer_root_path.to_string(),
        inline_password: inline_password.map(str::to_string),
        url_timeout_conn_seconds,
        global_timeout_conn_seconds,
        run_mode,
        home_directory,
        known_hosts,
        ssh_agent_socket: None,
    }
}

fn workspace_root() -> PathBuf {
    let mut current = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if current.join("aitc").is_dir() && current.join("extart").is_dir() {
            return current;
        }
        assert!(current.pop(), "could not locate workspace root");
    }
}

fn uv_path(root: &Path) -> PathBuf {
    let binary = if cfg!(windows) {
        "uv.exe"
    } else if cfg!(target_os = "macos") {
        "uv.mac"
    } else {
        "uv.linux"
    };
    root.join("aitc").join("bin").join(binary)
}
