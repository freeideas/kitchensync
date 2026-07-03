use ssh2::{CheckResult, FileStat, KnownHostFileKind, Session};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
QyNTUxOQAAACAhuMO8by9HNFpXlbtItwY6N3tl18y6dmuiqcvhl8dzRgAAAJivdUh1r3VI\n\
dQAAAAtzc2gtZWQyNTUxOQAAACAhuMO8by9HNFpXlbtItwY6N3tl18y6dmuiqcvhl8dzRg\n\
AAAEALRF/BTksyYA5wJjMqgnjDh9my9NN9Ecr91X3UGbpB7yG4w7xvL0c0WleVu0i3Bjo3\n\
e2XXzLp2a6Kpy+GXx3NGAAAAEGtpdGNoZW5zeW5jLXBsYW4BAgMEBQ==\n\
-----END OPENSSH PRIVATE KEY-----\n";

const PUBLIC_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICG4w7xvL0c0WleVu0i3Bjo3e2XXzLp2a6Kpy+GXx3NG kitchensync-plan\n";

struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn workspace_root() -> Result<PathBuf, Box<dyn Error>> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("extart/ephemeral-sftp-server.py").is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("could not find workspace root".into());
        }
    }
}

fn temp_root() -> Result<PathBuf, Box<dyn Error>> {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kitchensync-sftp-transport-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn start_server(root: &Path, workspace: &Path) -> Result<(Server, u16, String), Box<dyn Error>> {
    let key_dir = root.join("keys");
    fs::create_dir_all(&key_dir)?;
    let private_path = key_dir.join("id_ed25519");
    let public_path = key_dir.join("id_ed25519.pub");
    fs::write(&private_path, PRIVATE_KEY)?;
    fs::write(&public_path, PUBLIC_KEY)?;

    let uv = workspace.join("aisf/bin/uv.linux");
    let script = workspace.join("extart/ephemeral-sftp-server.py");
    let mut child = Command::new(uv)
        .arg("run")
        .arg("--script")
        .arg(script)
        .arg("--user")
        .arg("plan")
        .arg("--authorized-key")
        .arg(&public_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdout = BufReader::new(child.stdout.take().ok_or("missing server stdout")?);
    let mut port_line = String::new();
    stdout.read_line(&mut port_line)?;
    let port: u16 = port_line.trim().parse()?;

    let mut stderr = BufReader::new(child.stderr.take().ok_or("missing server stderr")?);
    let mut host_key = String::new();
    for _ in 0..8 {
        let mut line = String::new();
        stderr.read_line(&mut line)?;
        if let Some(rest) = line.strip_prefix("host key: ") {
            host_key = rest.trim().to_string();
            break;
        }
    }
    assert!(!host_key.is_empty(), "server did not print a host key line");

    Ok((Server { child }, port, host_key))
}

fn connect_with_known_host(
    port: u16,
    host_key: &str,
    known_hosts: &Path,
) -> Result<Session, Box<dyn Error>> {
    fs::write(
        known_hosts,
        format!("[127.0.0.1]:{} {}\n", port, host_key),
    )?;

    let tcp = TcpStream::connect(("127.0.0.1", port))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.set_timeout(10_000);
    session.handshake()?;

    let (remote_key, _) = session.host_key().ok_or("server did not provide host key")?;
    let mut known = session.known_hosts()?;
    assert!(
        matches!(
            known.check_port("127.0.0.1", port, remote_key),
            CheckResult::NotFound
        ),
        "empty known-host set must not trust the server"
    );
    known.read_file(known_hosts, KnownHostFileKind::OpenSSH)?;
    assert!(
        matches!(
            known.check_port("127.0.0.1", port, remote_key),
            CheckResult::Match
        ),
        "pinned known-host line must match the server"
    );
    Ok(session)
}

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = workspace_root()?;
    let root = temp_root()?;
    let (server, port, host_key) = start_server(&root, &workspace)?;
    let known_hosts = root.join("known_hosts");
    let private_path = root.join("keys/id_ed25519");
    let public_path = root.join("keys/id_ed25519.pub");

    let session = connect_with_known_host(port, &host_key, &known_hosts)?;
    assert!(session.userauth_password("plan", "wrong").is_err());
    assert!(!session.authenticated());
    session.userauth_pubkey_file("plan", Some(&public_path), &private_path, None)?;
    assert!(session.authenticated());

    let sftp = session.sftp()?;
    sftp.mkdir(Path::new("/alpha"), 0o755)?;

    let file_path = Path::new("/alpha/file.txt");
    {
        let mut remote = sftp.create(file_path)?;
        remote.write_all(b"hello over sftp")?;
    }
    let stat = sftp.stat(file_path)?;
    assert_eq!(stat.size, Some(15));

    sftp.setstat(
        file_path,
        FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: None,
            atime: Some(1_704_110_400),
            mtime: Some(1_704_110_400),
        },
    )?;
    assert_eq!(sftp.stat(file_path)?.mtime, Some(1_704_110_400));

    let mut text = String::new();
    sftp.open(file_path)?.read_to_string(&mut text)?;
    assert_eq!(text, "hello over sftp");

    let names: Vec<String> = sftp
        .readdir(Path::new("/alpha"))?
        .into_iter()
        .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|name| name == "file.txt"));

    let renamed = Path::new("/alpha/renamed.txt");
    sftp.rename(file_path, renamed, None)?;
    assert!(sftp.stat(file_path).is_err());
    assert_eq!(sftp.stat(renamed)?.size, Some(15));

    let existing = Path::new("/alpha/existing.txt");
    let source = Path::new("/alpha/source.txt");
    {
        let mut remote = sftp.create(existing)?;
        remote.write_all(b"existing")?;
    }
    {
        let mut remote = sftp.create(source)?;
        remote.write_all(b"source")?;
    }
    sftp.rename(source, existing, None)?;
    let mut overwritten = String::new();
    sftp.open(existing)?.read_to_string(&mut overwritten)?;
    assert_eq!(overwritten, "source");

    drop(sftp);
    drop(session);
    drop(server);
    fs::remove_dir_all(root)?;

    println!("checked ssh2 known-host verification, Ed25519 key auth, SFTP read/write/stat/setstat/readdir/rename");
    Ok(())
}
