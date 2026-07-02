use commandandoutput_peeridentitynormalizer::{
    new, LocalPeerIdentityTarget, PeerIdentityNormalizer, PeerIdentityTarget,
    SftpPeerIdentityTarget,
};
use std::path::{Path, PathBuf};

fn normalize(
    subject: &dyn PeerIdentityNormalizer,
    target: PeerIdentityTarget,
    current_working_directory: PathBuf,
    current_os_username: &str,
) -> String {
    subject
        .normalize_peer_identity(
            target,
            current_working_directory,
            current_os_username.to_string(),
        )
        .expect("accepted peer target should normalize")
}

fn test_current_working_directory() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\kitchensync-peer-identity-cwd")
    }

    #[cfg(not(windows))]
    {
        PathBuf::from("/tmp/kitchensync-peer-identity-cwd")
    }
}

fn file_url_for_absolute_path(path: &Path) -> String {
    let path_text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        format!("file:///{path_text}")
    } else {
        format!("file://{path_text}")
    }
}

fn local(path_or_url: &str) -> PeerIdentityTarget {
    PeerIdentityTarget::Local(LocalPeerIdentityTarget {
        path_or_url: path_or_url.to_string(),
    })
}

fn sftp(
    host: &str,
    username: Option<&str>,
    port: u16,
    absolute_path: &str,
) -> PeerIdentityTarget {
    PeerIdentityTarget::Sftp(SftpPeerIdentityTarget {
        host: host.to_string(),
        username: username.map(str::to_string),
        port,
        absolute_path: absolute_path.to_string(),
    })
}

#[test]
fn bare_local_peer_path_becomes_file_url_identity() {
    let subject = new();
    let cwd = test_current_working_directory();

    let identity = normalize(&*subject, local("plain-peer"), cwd.clone(), "unused-user");

    assert_eq!(
        identity,
        file_url_for_absolute_path(&cwd.join("plain-peer"))
    );
}

#[test]
fn relative_local_peer_path_is_resolved_from_current_working_directory() {
    let subject = new();
    let cwd = test_current_working_directory();

    let identity = normalize(&*subject, local("incoming//peer/"), cwd.clone(), "unused-user");

    assert_eq!(
        identity,
        file_url_for_absolute_path(&cwd.join("incoming").join("peer"))
    );
}

#[test]
fn windows_drive_peer_path_becomes_file_url_identity() {
    let subject = new();

    let identity = normalize(
        &*subject,
        local(r"c:\Users\Alice\project//"),
        test_current_working_directory(),
        "unused-user",
    );

    assert_eq!(identity, "file:///C:/Users/Alice/project");
}

#[test]
fn file_peer_url_identity_normalizes_scheme_host_path_and_query() {
    let subject = new();

    let identity = normalize(
        &*subject,
        local("FILE://Example.COM//Team///%7Ealice/%2Fkept/?timeout=5"),
        test_current_working_directory(),
        "unused-user",
    );

    assert_eq!(identity, "file://example.com/Team/~alice/%2Fkept");
}

#[test]
fn sftp_identity_uses_current_user_and_removes_default_port() {
    let subject = new();

    let identity = normalize(
        &*subject,
        sftp("Example.COM", None, 22, "//docs///%7Ealice/"),
        test_current_working_directory(),
        "os-user",
    );

    assert_eq!(identity, "sftp://os-user@example.com/docs/~alice");
}

#[test]
fn sftp_identity_keeps_explicit_user_non_default_port_and_reserved_encoding() {
    let subject = new();

    let identity = normalize(
        &*subject,
        sftp(
            "Example.COM",
            Some("RemoteUser"),
            2222,
            "/team/%2Froot/%3Fquery/%7Ebob",
        ),
        test_current_working_directory(),
        "ignored-user",
    );

    assert_eq!(
        identity,
        "sftp://RemoteUser@example.com:2222/team/%2Froot/%3Fquery/~bob"
    );
}
