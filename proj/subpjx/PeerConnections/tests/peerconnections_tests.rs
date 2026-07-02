use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use peerconnections::{
    new as new_peer_connections, PeerConnectionDiagnosticKind,
    PeerConnectionEffectiveSftpSettings, PeerConnectionFatalStartupReason,
    PeerConnectionGlobalSettings, PeerConnectionLocalEnvironment,
    PeerConnectionLocalUrl, PeerConnectionLocation, PeerConnectionPeer,
    PeerConnectionPeerRole, PeerConnectionRunMode, PeerConnectionSftpUrl,
    PeerConnectionStartupRequest, PeerConnectionStartupStatus, PeerConnectionUrl,
    PeerConnectionUrlSettings, PeerConnections,
};
use peerconnections_fileurlconnection::new as new_file_url_connection;
use peerconnections_sftpurlconnection::new as new_sftp_url_connection;
use peerconnections_startupcoordinator::new as new_startup_coordinator;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "peerconnections-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct SftpServer {
    child: Child,
    port: u16,
}

impl Drop for SftpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct StalledTcpServer {
    port: u16,
    stop: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl StalledTcpServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled TCP server");
        listener
            .set_nonblocking(true)
            .expect("make stalled TCP server nonblocking");
        let port = listener
            .local_addr()
            .expect("read stalled TCP server address")
            .port();
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let mut streams = Vec::new();
            loop {
                if rx.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => streams.push(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            stop: Some(tx),
            handle: Some(handle),
        }
    }
}

impl Drop for StalledTcpServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("PeerConnections has a parent directory")
        .parent()
        .expect("subpjx has a parent directory")
        .parent()
        .expect("proj has a parent directory")
        .to_path_buf()
}

fn bundled_uv(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("aitc/bin/uv.exe")
    } else if cfg!(target_os = "macos") {
        root.join("aitc/bin/uv.mac")
    } else {
        root.join("aitc/bin/uv.linux")
    }
}

fn start_sftp_server(extra_args: &[String]) -> SftpServer {
    let root = workspace_root();
    let mut child = Command::new(bundled_uv(&root))
        .current_dir(&root)
        .arg("run")
        .arg("--script")
        .arg("extart/ephemeral-sftp-server.py")
        .args(extra_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start ephemeral SFTP server");

    let stdout = child.stdout.take().expect("server stdout is piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout)
            .read_line(&mut line)
            .map(|_| line);
        let _ = tx.send(result);
    });

    let port_line = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(line)) => line,
        Ok(Err(error)) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("read SFTP server port: {error}");
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("SFTP server did not print its port");
        }
    };
    let port = port_line.trim().parse().expect("SFTP server port is numeric");

    SftpServer { child, port }
}

fn subject() -> std::sync::Arc<dyn PeerConnections> {
    let file_url_connection = new_file_url_connection();
    let sftp_url_connection = new_sftp_url_connection();
    let startup_coordinator =
        new_startup_coordinator(file_url_connection.clone(), sftp_url_connection.clone());
    new_peer_connections(file_url_connection, sftp_url_connection, startup_coordinator)
}

fn empty_local_environment(home: &Path) -> PeerConnectionLocalEnvironment {
    let known_hosts = home.join(".ssh").join("known_hosts");
    PeerConnectionLocalEnvironment {
        home_directory: home.to_path_buf(),
        known_hosts_path: known_hosts,
        ssh_agent_socket: None,
    }
}

fn local_url(identity: &str, path: &Path) -> PeerConnectionUrl {
    PeerConnectionUrl {
        normalized_identity: identity.to_string(),
        location: PeerConnectionLocation::Local(PeerConnectionLocalUrl {
            path_or_url: path.to_string_lossy().into_owned(),
        }),
        connection: PeerConnectionUrlSettings {
            timeout_conn_seconds: Some(1),
            timeout_idle_seconds: Some(1),
        },
    }
}

fn sftp_url(
    identity: &str,
    port: u16,
    password: Option<&str>,
    absolute_path: &str,
    timeout_conn_seconds: Option<u32>,
    timeout_idle_seconds: Option<u32>,
) -> PeerConnectionUrl {
    PeerConnectionUrl {
        normalized_identity: identity.to_string(),
        location: PeerConnectionLocation::Sftp(PeerConnectionSftpUrl {
            host: "127.0.0.1".to_string(),
            username: "alice".to_string(),
            password: password.map(str::to_string),
            port,
            absolute_path: absolute_path.to_string(),
        }),
        connection: PeerConnectionUrlSettings {
            timeout_conn_seconds,
            timeout_idle_seconds,
        },
    }
}

fn request(
    peers: Vec<PeerConnectionPeer>,
    run_mode: PeerConnectionRunMode,
    home: &Path,
) -> PeerConnectionStartupRequest {
    request_with_global(peers, run_mode, home, 23, 41)
}

fn request_with_global(
    peers: Vec<PeerConnectionPeer>,
    run_mode: PeerConnectionRunMode,
    home: &Path,
    timeout_conn_seconds: u32,
    timeout_idle_seconds: u32,
) -> PeerConnectionStartupRequest {
    PeerConnectionStartupRequest {
        peers,
        global_connection: PeerConnectionGlobalSettings {
            timeout_conn_seconds,
            timeout_idle_seconds,
        },
        run_mode,
        local_environment: empty_local_environment(home),
    }
}

fn write_known_hosts(home: &Path, port: u16) {
    let ssh_dir = home.join(".ssh");
    fs::create_dir_all(&ssh_dir).expect("create .ssh directory");
    let public_key = fs::read_to_string(
        workspace_root()
            .join("plan")
            .join("experiments")
            .join("sftp-client")
            .join("id_ed25519.pub"),
    )
    .expect("read fixture public key");
    fs::write(
        ssh_dir.join("known_hosts"),
        format!("[127.0.0.1]:{port} {}\n", public_key.trim()),
    )
    .expect("write known_hosts");
}

fn install_ed25519_identity(home: &Path) {
    let ssh_dir = home.join(".ssh");
    fs::create_dir_all(&ssh_dir).expect("create .ssh directory");
    let root = workspace_root();
    fs::copy(
        root.join("plan/experiments/sftp-client/id_ed25519"),
        ssh_dir.join("id_ed25519"),
    )
    .expect("install private key");
    fs::copy(
        root.join("plan/experiments/sftp-client/id_ed25519.pub"),
        ssh_dir.join("id_ed25519.pub"),
    )
    .expect("install public key");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(ssh_dir.join("id_ed25519"), fs::Permissions::from_mode(0o600))
            .expect("restrict private key permissions");
    }
}

#[test]
fn file_peer_selects_first_successful_url_and_does_not_try_later_fallbacks() {
    let temp = TestDir::new("file-fallbacks");
    let home = temp.path().join("home");
    let blocked_primary = temp.path().join("blocked-primary");
    let winning_fallback = temp.path().join("winning").join("root");
    let untried_fallback = temp.path().join("untried").join("root");
    let other_peer = temp.path().join("other");

    fs::create_dir_all(&home).expect("create home directory");
    fs::write(&blocked_primary, "not a directory").expect("create blocked primary");
    fs::create_dir_all(&other_peer).expect("create other peer root");

    let result = subject().establish_peer_connections(request(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![
                    local_url("file://blocked-primary", &blocked_primary),
                    local_url("file://winning", &winning_fallback),
                    local_url("file://untried", &untried_fallback),
                ],
            },
            PeerConnectionPeer {
                identity: "subordinate".to_string(),
                role: PeerConnectionPeerRole::Subordinate,
                urls: vec![local_url("file://other", &other_peer)],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
    ));

    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert!(result.unreachable_peers.is_empty());
    assert_eq!(result.reachable_peers.len(), 2);
    assert_eq!(result.reachable_peers[0].peer_identity, "canon");
    assert_eq!(
        result.reachable_peers[0].winning_url.normalized_identity,
        "file://winning"
    );
    assert_eq!(result.reachable_peers[0].effective_sftp_connection, None);
    assert!(winning_fallback.is_dir());
    assert!(!untried_fallback.exists());
    assert!(blocked_primary.is_file());
}

#[test]
fn dry_run_missing_file_peer_is_unreachable_and_reports_fatal_startup() {
    let temp = TestDir::new("dry-run-unreachable");
    let home = temp.path().join("home");
    let canon_root = temp.path().join("canon");
    let missing_root = temp.path().join("missing").join("root");

    fs::create_dir_all(&home).expect("create home directory");
    fs::create_dir_all(&canon_root).expect("create canon root");

    let result = subject().establish_peer_connections(request(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![local_url("file://canon", &canon_root)],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![local_url("file://missing", &missing_root)],
            },
        ],
        PeerConnectionRunMode::DryRun,
        &home,
    ));

    assert!(!missing_root.exists());
    assert_eq!(result.reachable_peers.len(), 1);
    assert_eq!(result.unreachable_peers.len(), 1);
    assert_eq!(result.unreachable_peers[0].peer_identity, "normal");
    assert_eq!(
        result.unreachable_peers[0].diagnostic.kind,
        PeerConnectionDiagnosticKind::UnreachablePeer
    );
    assert_eq!(
        result.status,
        PeerConnectionStartupStatus::Fatal(vec![
            PeerConnectionFatalStartupReason::FewerThanTwoReachablePeers
        ])
    );
}

#[test]
fn unreachable_canon_peer_is_a_fatal_startup_reason() {
    let temp = TestDir::new("canon-unreachable");
    let home = temp.path().join("home");
    let missing_canon = temp.path().join("missing-canon");
    let normal_root = temp.path().join("normal");

    fs::create_dir_all(&home).expect("create home directory");
    fs::create_dir_all(&normal_root).expect("create normal root");

    let result = subject().establish_peer_connections(request(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![local_url("file://missing-canon", &missing_canon)],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![local_url("file://normal", &normal_root)],
            },
        ],
        PeerConnectionRunMode::DryRun,
        &home,
    ));

    assert_eq!(result.unreachable_peers.len(), 1);
    assert_eq!(result.unreachable_peers[0].peer_identity, "canon");
    match result.status {
        PeerConnectionStartupStatus::Fatal(reasons) => {
            assert_eq!(reasons.len(), 2);
            assert!(reasons.contains(&PeerConnectionFatalStartupReason::FewerThanTwoReachablePeers));
            assert!(reasons.contains(&PeerConnectionFatalStartupReason::CanonPeerUnreachable));
        }
        PeerConnectionStartupStatus::Ready => panic!("startup should be fatal"),
    }
}

#[test]
fn sftp_winners_report_effective_timeouts_and_create_remote_roots() {
    let temp = TestDir::new("sftp-timeouts");
    let home = temp.path().join("home");
    let host_key = workspace_root().join("plan/experiments/sftp-client/id_ed25519");
    let server = start_sftp_server(&[
        "--user".to_string(),
        "alice".to_string(),
        "--password".to_string(),
        "secret".to_string(),
        "--host-key".to_string(),
        host_key.to_string_lossy().into_owned(),
    ]);
    write_known_hosts(&home, server.port);

    let result = subject().establish_peer_connections(request(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![sftp_url(
                    "sftp://canon",
                    server.port,
                    Some("secret"),
                    "/created/by/url-settings",
                    Some(7),
                    Some(11),
                )],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![sftp_url(
                    "sftp://normal",
                    server.port,
                    Some("secret"),
                    "/created/by/global-settings",
                    None,
                    None,
                )],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
    ));

    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert!(result.unreachable_peers.is_empty());
    assert_eq!(result.reachable_peers.len(), 2);
    assert_eq!(
        result.reachable_peers[0].effective_sftp_connection,
        Some(PeerConnectionEffectiveSftpSettings {
            timeout_conn_seconds: 7,
            timeout_idle_seconds: 11,
        })
    );
    assert_eq!(
        result.reachable_peers[1].effective_sftp_connection,
        Some(PeerConnectionEffectiveSftpSettings {
            timeout_conn_seconds: 23,
            timeout_idle_seconds: 41,
        })
    );
}

#[test]
fn sftp_without_url_timeout_uses_global_timeout_before_trying_fallback() {
    let temp = TestDir::new("sftp-global-timeout");
    let home = temp.path().join("home");
    let stalled = StalledTcpServer::new();
    let fallback_root = temp.path().join("fallback");
    let normal_root = temp.path().join("normal");

    fs::create_dir_all(&home).expect("create home directory");
    fs::create_dir_all(&normal_root).expect("create normal root");

    let started = Instant::now();
    let result = subject().establish_peer_connections(request_with_global(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![
                    sftp_url("sftp://stalled", stalled.port, Some("secret"), "/", None, None),
                    local_url("file://fallback", &fallback_root),
                ],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![local_url("file://normal", &normal_root)],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
        1,
        41,
    ));

    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert_eq!(
        result.reachable_peers[0].winning_url.normalized_identity,
        "file://fallback"
    );
    assert!(fallback_root.is_dir());
}

#[test]
fn sftp_url_timeout_overrides_global_timeout_before_trying_fallback() {
    let temp = TestDir::new("sftp-url-timeout");
    let home = temp.path().join("home");
    let stalled = StalledTcpServer::new();
    let fallback_root = temp.path().join("fallback");
    let normal_root = temp.path().join("normal");

    fs::create_dir_all(&home).expect("create home directory");
    fs::create_dir_all(&normal_root).expect("create normal root");

    let started = Instant::now();
    let result = subject().establish_peer_connections(request_with_global(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![
                    sftp_url(
                        "sftp://stalled-url-timeout",
                        stalled.port,
                        Some("secret"),
                        "/",
                        Some(1),
                        None,
                    ),
                    local_url("file://fallback", &fallback_root),
                ],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![local_url("file://normal", &normal_root)],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
        3,
        41,
    ));

    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert_eq!(
        result.reachable_peers[0].winning_url.normalized_identity,
        "file://fallback"
    );
    assert!(fallback_root.is_dir());
}

#[test]
fn startup_begins_connection_work_for_all_peers_in_parallel() {
    let temp = TestDir::new("parallel-startup");
    let home = temp.path().join("home");
    let canon_root = temp.path().join("canon");
    let normal_root = temp.path().join("normal");
    let stalled_one = StalledTcpServer::new();
    let stalled_two = StalledTcpServer::new();

    fs::create_dir_all(&home).expect("create home directory");
    fs::create_dir_all(&canon_root).expect("create canon root");
    fs::create_dir_all(&normal_root).expect("create normal root");

    let started = Instant::now();
    let result = subject().establish_peer_connections(request_with_global(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![local_url("file://canon", &canon_root)],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![local_url("file://normal", &normal_root)],
            },
            PeerConnectionPeer {
                identity: "stalled-one".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![sftp_url(
                    "sftp://stalled-one",
                    stalled_one.port,
                    Some("secret"),
                    "/",
                    None,
                    None,
                )],
            },
            PeerConnectionPeer {
                identity: "stalled-two".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![sftp_url(
                    "sftp://stalled-two",
                    stalled_two.port,
                    Some("secret"),
                    "/",
                    None,
                    None,
                )],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
        1,
        41,
    ));

    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert_eq!(result.reachable_peers.len(), 2);
    assert_eq!(result.unreachable_peers.len(), 2);
}

#[test]
fn sftp_unknown_host_key_fails_only_that_peer() {
    let temp = TestDir::new("sftp-host-key");
    let home = temp.path().join("home");
    let canon_root = temp.path().join("canon");
    let normal_root = temp.path().join("normal");
    let host_key = workspace_root().join("plan/experiments/sftp-client/id_ed25519");
    let server = start_sftp_server(&[
        "--user".to_string(),
        "alice".to_string(),
        "--password".to_string(),
        "secret".to_string(),
        "--host-key".to_string(),
        host_key.to_string_lossy().into_owned(),
    ]);

    fs::create_dir_all(home.join(".ssh")).expect("create .ssh directory");
    fs::write(home.join(".ssh").join("known_hosts"), "").expect("write empty known_hosts");
    fs::create_dir_all(&canon_root).expect("create canon root");
    fs::create_dir_all(&normal_root).expect("create normal root");

    let result = subject().establish_peer_connections(request(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![local_url("file://canon", &canon_root)],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![local_url("file://normal", &normal_root)],
            },
            PeerConnectionPeer {
                identity: "sftp".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![sftp_url(
                    "sftp://untrusted",
                    server.port,
                    Some("secret"),
                    "/host-key-rejected",
                    Some(3),
                    Some(5),
                )],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
    ));

    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert_eq!(result.reachable_peers.len(), 2);
    assert_eq!(result.unreachable_peers.len(), 1);
    assert_eq!(result.unreachable_peers[0].peer_identity, "sftp");
    assert_eq!(
        result.unreachable_peers[0].diagnostic.kind,
        PeerConnectionDiagnosticKind::UnreachablePeer
    );
}

#[test]
fn sftp_authentication_continues_from_rejected_inline_password_to_ed25519_key() {
    let temp = TestDir::new("sftp-auth-fallback");
    let home = temp.path().join("home");
    let key = workspace_root().join("plan/experiments/sftp-client/id_ed25519");
    let public_key = workspace_root().join("plan/experiments/sftp-client/id_ed25519.pub");
    let server = start_sftp_server(&[
        "--user".to_string(),
        "alice".to_string(),
        "--authorized-key".to_string(),
        public_key.to_string_lossy().into_owned(),
        "--host-key".to_string(),
        key.to_string_lossy().into_owned(),
    ]);
    write_known_hosts(&home, server.port);
    install_ed25519_identity(&home);

    let result = subject().establish_peer_connections(request(
        vec![
            PeerConnectionPeer {
                identity: "canon".to_string(),
                role: PeerConnectionPeerRole::Canon,
                urls: vec![sftp_url(
                    "sftp://canon-key",
                    server.port,
                    Some("wrong-password"),
                    "/auth/fallback/canon",
                    Some(9),
                    Some(13),
                )],
            },
            PeerConnectionPeer {
                identity: "normal".to_string(),
                role: PeerConnectionPeerRole::Normal,
                urls: vec![sftp_url(
                    "sftp://normal-key",
                    server.port,
                    None,
                    "/auth/fallback/normal",
                    None,
                    None,
                )],
            },
        ],
        PeerConnectionRunMode::Normal,
        &home,
    ));

    assert_eq!(result.status, PeerConnectionStartupStatus::Ready);
    assert!(result.unreachable_peers.is_empty());
    assert_eq!(result.reachable_peers.len(), 2);
    assert_eq!(
        result.reachable_peers[0].winning_url.normalized_identity,
        "sftp://canon-key"
    );
    assert_eq!(
        result.reachable_peers[1].winning_url.normalized_identity,
        "sftp://normal-key"
    );
}
