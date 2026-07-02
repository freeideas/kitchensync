use ssh2::{CheckResult, FileStat, KnownHostFileKind, Session};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

fn workspace_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("current dir");
    for _ in 0..3 {
        dir = dir.parent().expect("workspace parent").to_path_buf();
    }
    dir
}

fn start_server(args: &[&str]) -> Result<(Child, u16, String), Box<dyn Error>> {
    let root = workspace_root();
    let mut command = Command::new(root.join("aitc/bin/uv.linux"));
    command
        .arg("run")
        .arg("--script")
        .arg(root.join("extart/ephemeral-sftp-server.py"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("server stdout");
    let mut out = BufReader::new(stdout);
    let mut port_line = String::new();
    out.read_line(&mut port_line)?;
    let port: u16 = port_line.trim().parse()?;

    let stderr = child.stderr.take().expect("server stderr");
    let mut err = BufReader::new(stderr);
    let mut host_key = String::new();
    let mut line = String::new();
    while err.read_line(&mut line)? != 0 {
        if let Some(rest) = line.trim().strip_prefix("host key: ") {
            host_key = rest.to_string();
            break;
        }
        line.clear();
    }
    assert!(!host_key.is_empty(), "server did not print host key");
    thread::spawn(move || {
        for line in err.lines() {
            if line.is_err() {
                break;
            }
        }
    });
    Ok((child, port, host_key))
}

fn connect_checked(port: u16, host_key: &str) -> Result<Session, Box<dyn Error>> {
    let tcp = TcpStream::connect(("127.0.0.1", port))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;
    let (raw_key, _) = session.host_key().expect("session host key");
    let mut known_hosts = session.known_hosts()?;
    let known_line = format!("[127.0.0.1]:{} {}", port, host_key);
    known_hosts.read_str(&known_line, KnownHostFileKind::OpenSSH)?;
    assert!(matches!(
        known_hosts.check_port("127.0.0.1", port, raw_key),
        CheckResult::Match
    ));
    Ok(session)
}

fn password_auth_and_file_ops() -> Result<(), Box<dyn Error>> {
    let (mut server, port, host_key) =
        start_server(&["--user", "alice", "--password", "secret"])?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut session = connect_checked(port, &host_key)?;
        session.userauth_password("alice", "secret")?;
        assert!(session.authenticated());
        let sftp = session.sftp()?;

        sftp.mkdir(Path::new("/alpha"), 0o755)?;
        let mut writer = sftp.create(Path::new("/alpha/new"))?;
        writer.write_all(b"hello over sftp")?;
        drop(writer);

        let stat = sftp.stat(Path::new("/alpha/new"))?;
        assert_eq!(stat.size, Some(15));

        sftp.setstat(
            Path::new("/alpha/new"),
            FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: None,
                atime: Some(1_700_000_000),
                mtime: Some(1_700_000_123),
            },
        )?;
        assert_eq!(
            sftp.stat(Path::new("/alpha/new"))?.mtime,
            Some(1_700_000_123)
        );

        sftp.rename(Path::new("/alpha/new"), Path::new("/alpha/final"), None)?;
        let mut reader = sftp.open(Path::new("/alpha/final"))?;
        let mut body = String::new();
        reader.read_to_string(&mut body)?;
        assert_eq!(body, "hello over sftp");

        let names: Vec<String> = sftp
            .readdir(Path::new("/alpha"))?
            .into_iter()
            .filter_map(|(path, _)| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .collect();
        assert_eq!(names, vec!["final".to_string()]);
        sftp.unlink(Path::new("/alpha/final"))?;
        sftp.rmdir(Path::new("/alpha"))?;
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

fn ed25519_public_key_auth() -> Result<(), Box<dyn Error>> {
    let root = std::env::current_dir()?;
    let private_key = root.join("id_ed25519");
    let public_key = root.join("id_ed25519.pub");
    assert!(private_key.is_file());
    assert!(public_key.is_file());
    let (mut server, port, host_key) = start_server(&[
        "--user",
        "alice",
        "--authorized-key",
        public_key.to_str().expect("public key path"),
    ])?;
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut session = connect_checked(port, &host_key)?;
        session.userauth_pubkey_file("alice", None, &private_key, None)?;
        assert!(session.authenticated());
        let sftp = session.sftp()?;
        sftp.mkdir(Path::new("/keyauth"), 0o755)?;
        sftp.rmdir(Path::new("/keyauth"))?;
        Ok(())
    })();
    let _ = server.kill();
    let _ = server.wait();
    result
}

fn main() -> Result<(), Box<dyn Error>> {
    fs::set_permissions("id_ed25519", {
        let mut permissions = fs::metadata("id_ed25519")?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o600);
        }
        permissions
    })?;
    password_auth_and_file_ops()?;
    ed25519_public_key_auth()?;
    println!("checked ssh2 host-key verification, password auth, Ed25519 key auth, and SFTP file calls");
    Ok(())
}
