use std::env;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use kitchensync::{
    peer::{connect_peers, resolve_roles, PeerStartupError, PendingPeerSession, SnapshotExistence},
    DiagnosticEvent, DiagnosticSink, EffectivePeerRole, EntryMeta, RelPath, RunConfig, Timestamp,
    TransportBackend, TransportError, TransportFactory, TransportHandle, TransportRead, TransportTimeouts,
    TransportRootMode, Verbosity, PeerRole, PeerSpec, PeerUrl,
};

#[derive(Clone)]
struct FakeTransportFactory {
    policy: Arc<dyn Fn(&PeerUrl, TransportTimeouts, TransportRootMode) -> Result<(), TransportError> + Send + Sync>,
    calls: Arc<Mutex<Vec<ConnectCall>>> ,
}

#[derive(Clone, Debug)]
struct ConnectCall {
    url: PeerUrl,
    timeouts: TransportTimeouts,
    root_mode: TransportRootMode,
}

impl FakeTransportFactory {
    fn new<F>(policy: F) -> Self
    where
        F: Fn(&PeerUrl, TransportTimeouts, TransportRootMode) -> Result<(), TransportError>
            + Send
            + Sync
            + 'static,
    {
        Self {
            policy: Arc::new(policy),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<ConnectCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl TransportFactory for FakeTransportFactory {
    fn connect(
        &self,
        url: &PeerUrl,
        timeouts: TransportTimeouts,
        root_mode: TransportRootMode,
    ) -> Result<TransportHandle, TransportError> {
        self.calls.lock().unwrap().push(ConnectCall {
            url: url.clone(),
            timeouts,
            root_mode,
        });

        (self.policy)(url, timeouts, root_mode).map(|_| TransportHandle::new(NoopBackend))
    }
}

#[derive(Default)]
struct RecordingDiagnostics {
    messages: Arc<Mutex<Vec<String>>>,
}

impl RecordingDiagnostics {
    fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }
}

impl DiagnosticSink for RecordingDiagnostics {
    fn publish(&self, event: DiagnosticEvent) {
        if let DiagnosticEvent::Error { message } = event {
            self.messages.lock().unwrap().push(message);
        }
    }
}

#[derive(Clone)]
struct NoopBackend;

impl TransportBackend for NoopBackend {
    fn list_dir(&self, _path: &RelPath) -> Result<Vec<EntryMeta>, TransportError> {
        Err(TransportError::NotFound)
    }

    fn stat(&self, _path: &RelPath) -> Result<EntryMeta, TransportError> {
        Err(TransportError::NotFound)
    }

    fn open_read(&self, _path: &RelPath) -> Result<TransportRead, TransportError> {
        Err(TransportError::IoError)
    }

    fn open_write(&self, _path: &RelPath) -> Result<kitchensync::TransportWrite, TransportError> {
        Err(TransportError::IoError)
    }

    fn rename_no_overwrite(&self, _src: &RelPath, _dst: &RelPath) -> Result<(), TransportError> {
        Err(TransportError::PermissionDenied)
    }

    fn delete_file(&self, _path: &RelPath) -> Result<(), TransportError> {
        Err(TransportError::IoError)
    }

    fn create_dir(&self, _path: &RelPath) -> Result<(), TransportError> {
        Err(TransportError::PermissionDenied)
    }

    fn delete_dir(&self, _path: &RelPath) -> Result<(), TransportError> {
        Err(TransportError::PermissionDenied)
    }

    fn set_mod_time(&self, _path: &RelPath, _time: Timestamp) -> Result<(), TransportError> {
        Err(TransportError::IoError)
    }
}

fn peer_url(
    scheme: &str,
    username: Option<&str>,
    password: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    path: &str,
    timeout_conn: Option<u32>,
    timeout_idle: Option<u32>,
) -> PeerUrl {
    PeerUrl {
        scheme: scheme.to_string(),
        username: username.map(str::to_string),
        password: password.map(str::to_string),
        host: host.map(str::to_string),
        port,
        path: path.to_string(),
        identity: String::new(),
        timeout_conn,
        timeout_idle,
    }
}

fn run_config(dry_run: bool) -> RunConfig {
    RunConfig {
        dry_run,
        max_copies: 1,
        retries_copy: 0,
        retries_list: 0,
        timeout_conn: 30,
        timeout_idle: 40,
        verbosity: Verbosity::Error,
        keep_tmp_days: 7,
        keep_bak_days: 7,
        keep_del_days: 7,
        excludes: Vec::new(),
    }
}

fn current_user() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn pending_session(id: usize, declared_role: PeerRole, normalized_path: &str, selected_path: &str) -> PendingPeerSession {
    PendingPeerSession {
        id: id as u64,
        invocation_index: id,
        normalized_identity: peer_url("file", None, None, None, None, normalized_path, None, None),
        selected_url: peer_url("file", None, None, None, None, selected_path, None, None),
        declared_role,
        transport: TransportHandle::new(NoopBackend),
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    use std::ptr;

    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(ptr::null(), &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);

    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }

        std::thread::yield_now();
    }
}

#[test]
fn connect_peers_prefers_first_usable_fallback_url_for_a_peer() {
    let factory = FakeTransportFactory::new(|url, _, _| match url.path.as_str() {
        "/first-primary" => Err(TransportError::NotFound),
        "/first-fallback" => Ok(()),
        "/second-root" => Ok(()),
        _ => Err(TransportError::PermissionDenied),
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![
                peer_url("sftp", Some("alice"), Some("pw"), Some("Example.Com"), Some(22), "/first-primary", None, None),
                peer_url("SFTP", Some("alice"), Some("pw"), Some("Example.Com"), Some(22), "/first-fallback", None, None),
            ],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", Some("bob"), None, Some("other.example.com"), Some(22), "/second-root", None, None)],
        },
    ];

    let sessions = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics))
        .expect("two peers should connect");

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, 0);
    assert_eq!(sessions[1].id, 1);
    assert_eq!(sessions[0].invocation_index, 0);
    assert_eq!(sessions[1].invocation_index, 1);
    assert_eq!(sessions[0].selected_url.path, "/first-fallback");
    assert_eq!(sessions[1].selected_url.path, "/second-root");

    let calls = factory.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().any(|call| call.url.path == "/first-primary"));
    assert!(calls.iter().any(|call| call.url.path == "/first-fallback"));
    assert!(calls.iter().all(|call| call.root_mode == TransportRootMode::CreateMissing));
    assert!(diagnostics.messages().is_empty());
}

#[test]
fn connect_peers_normalizes_and_preserves_connection_identity() {
    let configured_user = current_user();
    let expected_user = configured_user.clone();
    let factory = FakeTransportFactory::new(move |url, _, _| {
        if url.username.as_deref() == Some(expected_user.as_str())
            && url.host.as_deref() == Some("example.com")
            && url.path == "/repo/~space/test"
        {
            Ok(())
        } else {
            Err(TransportError::NotFound)
        }
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![PeerSpec {
        role: PeerRole::Normal,
        urls: vec![peer_url(
            "SFTP",
            None,
            Some("secret"),
            Some("EXAMPLE.COM"),
            Some(22),
            "/repo//%7Espace/test/",
            Some(8),
            Some(16),
        )],
    }];

    let sessions = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics))
        .expect("peer should connect");
    let session = &sessions[0];

    assert_eq!(session.selected_url.scheme, "sftp");
    assert_eq!(session.selected_url.host.as_deref(), Some("example.com"));
    assert_eq!(session.selected_url.username.as_deref(), Some(configured_user.as_str()));
    assert_eq!(session.selected_url.port, None);
    assert_eq!(session.selected_url.path, "/repo/~space/test");
    assert_eq!(session.selected_url.timeout_conn, Some(8));
    assert_eq!(session.selected_url.timeout_idle, Some(16));
    assert_eq!(session.selected_url.password.as_deref(), Some("secret"));
    assert_eq!(
        session.selected_url.identity,
        format!("sftp://{}@example.com/repo/~space/test", configured_user)
    );

    assert_eq!(session.normalized_identity.password, None);
    assert_eq!(session.normalized_identity.timeout_conn, None);
    assert_eq!(session.normalized_identity.timeout_idle, None);
    assert_eq!(
        session.normalized_identity.identity,
        format!("sftp://{}@example.com/repo/~space/test", configured_user)
    );

    let calls = factory.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].timeouts.timeout_conn, 8);
    assert_eq!(calls[0].timeouts.timeout_idle, 16);
    assert_eq!(calls[0].root_mode, TransportRootMode::CreateMissing);
    assert!(diagnostics.messages().is_empty());
}

#[test]
fn connect_peers_reports_unreachable_peer_in_diagnostics() {
    let factory = FakeTransportFactory::new(|url, _, _| {
        if url.path == "/good-0" || url.path == "/good-1" {
            Ok(())
        } else {
            Err(TransportError::PermissionDenied)
        }
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("a.example.com"), Some(22), "/good-0", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("a.example.com"), Some(22), "/bad", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("a.example.com"), Some(22), "/good-1", None, None)],
        },
    ];

    let sessions = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics))
        .expect("two peers still reachable");

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, 0);
    assert_eq!(sessions[1].id, 2);
    assert_eq!(diagnostics.messages().len(), 1);
    assert!(diagnostics.messages()[0].contains("Peer 1 is unreachable"));
}

#[test]
fn connect_peers_rejects_declared_canon_unreachable() {
    let factory = FakeTransportFactory::new(|url, _, _| {
        if url.path == "/reachable-a" || url.path == "/reachable-b" {
            Ok(())
        } else {
            Err(TransportError::NotFound)
        }
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Canon,
            urls: vec![peer_url("sftp", None, None, Some("a.example.com"), Some(22), "/unreachable", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("a.example.com"), Some(22), "/reachable-a", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("a.example.com"), Some(22), "/reachable-b", None, None)],
        },
    ];

    let result = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics));

    assert!(matches!(
        result,
        Err(PeerStartupError::DeclaredCanonUnreachable { peer_id: 0 })
    ));
    assert_eq!(diagnostics.messages().len(), 1);
    assert!(diagnostics.messages()[0].contains("Peer 0 is unreachable"));
}

#[test]
fn connect_peers_rejects_when_fewer_than_two_peers_are_reachable() {
    let factory = FakeTransportFactory::new(|url, _, _| {
        if url.path == "/good" {
            Ok(())
        } else {
            Err(TransportError::NotFound)
        }
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("example.com"), Some(22), "/good", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("sftp", None, None, Some("example.com"), Some(22), "/bad", None, None)],
        },
    ];

    let result = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics));

    assert!(matches!(result, Err(PeerStartupError::TooFewReachablePeers)));
    assert_eq!(diagnostics.messages().len(), 1);
    assert!(diagnostics.messages()[0].contains("Peer 1 is unreachable"));
}

#[test]
fn connect_peers_uses_run_default_timeouts_when_peer_url_does_not_override() {
    let factory = FakeTransportFactory::new(|_, timeouts, _| {
        assert_eq!(timeouts.timeout_conn, 30);
        assert_eq!(timeouts.timeout_idle, 40);
        Ok(())
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url(
                "sftp",
                Some("alice"),
                None,
                Some("example.com"),
                None,
                "/first",
                None,
                None,
            )],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url(
                "sftp",
                Some("bob"),
                None,
                Some("example.com"),
                None,
                "/second",
                None,
                None,
            )],
        },
    ];

    let sessions = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics))
        .expect("both peers should connect");

    assert_eq!(sessions.len(), 2);
    assert!(diagnostics.messages().is_empty());
}

#[test]
fn connect_peers_converts_relative_file_peer_paths_to_absolute_identities() {
    let current_dir = env::current_dir().expect("working directory");
    let first_expected = current_dir
        .join("fixture-peer")
        .join("first")
        .to_string_lossy()
        .replace('\\', "/");
    let second_expected = current_dir
        .join("fixture-peer")
        .join("second")
        .to_string_lossy()
        .replace('\\', "/");

    let factory_first_expected = first_expected.clone();
    let factory_second_expected = second_expected.clone();
    let factory = FakeTransportFactory::new(move |url, _, _| {
        if url.path == factory_first_expected || url.path == factory_second_expected {
            Ok(())
        } else {
            Err(TransportError::NotFound)
        }
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("file", None, None, None, None, "fixture-peer/first", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("file", None, None, None, None, "fixture-peer/second", None, None)],
        },
    ];

    let sessions = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics))
        .expect("both peers should connect");

    assert_eq!(sessions[0].selected_url.path, first_expected);
    assert_eq!(sessions[1].selected_url.path, second_expected);
    assert_eq!(sessions[0].selected_url.scheme, "file");
    assert_eq!(sessions[1].selected_url.scheme, "file");
    assert_eq!(sessions[0].normalized_identity.path, first_expected);
    assert_eq!(sessions[1].normalized_identity.path, second_expected);
    assert!(diagnostics.messages().is_empty());
}

#[test]
fn connect_peers_uses_require_existing_root_mode_in_dry_run() {
    let factory = FakeTransportFactory::new(|url, _, root_mode| {
        if root_mode == TransportRootMode::RequireExisting {
            Ok(())
        } else if url.path == "/bad" {
            Err(TransportError::PermissionDenied)
        } else {
            Err(TransportError::NotFound)
        }
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("file", None, None, None, None, "/tmp/a", None, None)],
        },
        PeerSpec {
            role: PeerRole::Normal,
            urls: vec![peer_url("file", None, None, None, None, "/tmp/b", None, None)],
        },
    ];

    let sessions = block_on(connect_peers(&run_config(true), &peer_specs, &factory, &diagnostics))
        .expect("dry-run accepts existing roots");

    assert_eq!(sessions.len(), 2);
    assert!(factory
        .calls()
        .iter()
        .all(|call| call.root_mode == TransportRootMode::RequireExisting));
    assert!(diagnostics.messages().is_empty());
}

#[test]
fn resolve_roles_applies_effective_role_rules() {
    let pending = vec![
        pending_session(0, PeerRole::Normal, "/alpha", "/alpha"),
        pending_session(1, PeerRole::Normal, "/beta", "/beta"),
        pending_session(2, PeerRole::Subordinate, "/gamma", "/gamma"),
        pending_session(3, PeerRole::Canon, "/delta", "/delta"),
    ];

    let snapshots = vec![
        SnapshotExistence {
            peer_id: 0,
            existed: true,
        },
        SnapshotExistence {
            peer_id: 1,
            existed: false,
        },
        SnapshotExistence {
            peer_id: 3,
            existed: false,
        },
    ];

    let sessions = resolve_roles(pending, &snapshots).expect("should resolve roles");

    assert_eq!(sessions[0].effective_role, EffectivePeerRole::Contributing);
    assert_eq!(sessions[1].effective_role, EffectivePeerRole::Subordinate);
    assert_eq!(sessions[2].effective_role, EffectivePeerRole::Subordinate);
    assert_eq!(sessions[3].effective_role, EffectivePeerRole::Canon);
    assert!(sessions[0].had_startup_snapshot);
    assert!(!sessions[1].had_startup_snapshot);
    assert!(!sessions[2].had_startup_snapshot);
    assert_eq!(sessions[3].had_startup_snapshot, false);
    assert_eq!(sessions.iter().map(|session| session.id).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
}

#[test]
fn resolve_roles_requires_canon_for_first_sync() {
    let pending = vec![
        pending_session(0, PeerRole::Normal, "/alpha", "/alpha"),
        pending_session(1, PeerRole::Normal, "/beta", "/beta"),
    ];

    let err = resolve_roles(pending, &[]).expect_err("first sync requires canon");

    assert_eq!(err, PeerStartupError::FirstSyncNeedsCanon);
}

#[test]
fn resolve_roles_rejects_when_no_contributing_peer_remains() {
    let pending = vec![
        pending_session(0, PeerRole::Subordinate, "/alpha", "/alpha"),
        pending_session(1, PeerRole::Subordinate, "/beta", "/beta"),
    ];
    let snapshots = vec![
        SnapshotExistence {
            peer_id: 0,
            existed: true,
        },
        SnapshotExistence {
            peer_id: 1,
            existed: true,
        },
    ];

    let err = resolve_roles(pending, &snapshots).expect_err("subordinate peers cannot contribute");

    assert_eq!(err, PeerStartupError::NoContributingPeerReachable);
}

#[test]
fn peer_startup_error_messages_are_exact() {
    assert_eq!(
        PeerStartupError::FirstSyncNeedsCanon.to_string(),
        "First sync? Mark the authoritative peer with a leading +"
    );
    assert_eq!(
        PeerStartupError::NoContributingPeerReachable.to_string(),
        "No contributing peer reachable - cannot make sync decisions"
    );
}

#[test]
fn connect_peers_normalization_removes_query_from_normalized_identity_only() {
    let configured_user = current_user();
    let factory = FakeTransportFactory::new(|url, _, _| {
        assert_eq!(url.path, "/query/path?token=abc&mode=ro");
        Ok(())
    });
    let diagnostics = RecordingDiagnostics::default();

    let peer_specs = vec![PeerSpec {
        role: PeerRole::Normal,
        urls: vec![peer_url(
            "SFTP",
            None,
            None,
            Some("example.com"),
            Some(22),
            "/query%2Fpath?token=abc&mode=ro",
            None,
            None,
        )],
    }];

    let sessions = block_on(connect_peers(&run_config(false), &peer_specs, &factory, &diagnostics))
        .expect("peer should connect");
    let session = &sessions[0];

    assert_eq!(session.selected_url.path, "/query/path?token=abc&mode=ro");
    assert_eq!(session.normalized_identity.path, "/query/path");
    assert_eq!(session.selected_url.identity, format!("sftp://{}@example.com/query/path?token=abc&mode=ro", configured_user));
    assert_eq!(
        session.normalized_identity.identity,
        format!("sftp://{}@example.com/query/path", configured_user)
    );
    assert!(diagnostics.messages().is_empty());
}
