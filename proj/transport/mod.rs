use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    EntryKind, EntryMeta, PeerUrl, RelPath, Timestamp, TransportBackend, TransportError,
    TransportFactory, TransportHandle, TransportRead, TransportRootMode, TransportTimeouts,
    TransportWrite,
};

const PURPOSE: &str = "Local filesystem and SSH/SFTP file tree operations.";
const SUMMARY: &str = "transport: Local filesystem and SSH/SFTP file tree operations.";
const SFTP_KIND_MASK: u32 = 0o170000;
const SFTP_DIRECTORY: u32 = 0o040000;
const SFTP_REGULAR_FILE: u32 = 0o100000;
const SFTP_STATUS_NO_SUCH_FILE: i32 = 2;
const SFTP_STATUS_PERMISSION_DENIED: i32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportStatus {
    pub name: &'static str,
    pub purpose: &'static str,
}

pub fn status() -> TransportStatus {
    TransportStatus {
        name: "transport",
        purpose: PURPOSE,
    }
}

pub fn summary() -> &'static str {
    SUMMARY
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTransportFactory;

impl TransportFactory for DefaultTransportFactory {
    fn connect(
        &self,
        url: &PeerUrl,
        timeouts: TransportTimeouts,
        root_mode: TransportRootMode,
    ) -> Result<TransportHandle, TransportError> {
        match url.scheme.as_str() {
            "file" => connect_local(url, root_mode),
            "sftp" => connect_sftp(url, timeouts, root_mode)
                .or_else(|_| connect_openssh_sftp(url, root_mode)),
            _ => Err(TransportError::IoError),
        }
    }
}

impl DefaultTransportFactory {
    pub fn connect(
        &self,
        url: &PeerUrl,
        timeouts: TransportTimeouts,
        root_mode: TransportRootMode,
    ) -> Result<TransportHandle, TransportError> {
        TransportFactory::connect(self, url, timeouts, root_mode)
    }
}

pub fn factory() -> DefaultTransportFactory {
    DefaultTransportFactory
}

#[derive(Debug)]
struct LocalBackend {
    root: PathBuf,
}

impl TransportBackend for LocalBackend {
    fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError> {
        self.ensure_no_symlink_parents(path)?;
        let directory = self.resolve(path);
        ensure_local_directory(&directory)?;
        let entries = fs::read_dir(directory).map_err(map_io_error)?;
        let mut metas = Vec::new();

        for entry in entries {
            let entry = entry.map_err(map_io_error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = fs::symlink_metadata(entry.path()).map_err(map_io_error)?;
            if let Some(meta) = entry_meta_from_metadata(name, metadata) {
                metas.push(meta);
            }
        }

        Ok(metas)
    }

    fn stat(&self, path: &RelPath) -> Result<EntryMeta, TransportError> {
        self.ensure_no_symlink_parents(path)?;
        let full_path = self.resolve(path);
        let metadata = fs::symlink_metadata(&full_path).map_err(map_io_error)?;
        entry_meta_from_metadata(basename(path), metadata).ok_or(TransportError::NotFound)
    }

    fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError> {
        self.ensure_no_symlink_parents(path)?;
        let full_path = self.resolve(path);
        ensure_local_file(&full_path)?;
        let file = File::open(full_path).map_err(map_io_error)?;
        Ok(TransportRead::new(file))
    }

    fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError> {
        self.ensure_no_symlink_parents(path)?;
        let full_path = self.resolve(path);
        if fs::symlink_metadata(&full_path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(TransportError::NotFound);
        }
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(map_io_error)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(full_path)
            .map_err(map_io_error)?;
        Ok(TransportWrite::with_close(file, close_writer))
    }

    fn rename_no_overwrite(&self, src: &RelPath, dst: &RelPath) -> Result<(), TransportError> {
        self.ensure_no_symlink_parents(src)?;
        self.ensure_no_symlink_parents(dst)?;
        let src = self.resolve(src);
        let dst = self.resolve(dst);
        ensure_local_file_or_directory(&src)?;
        match fs::symlink_metadata(&dst) {
            Ok(_) => return Err(TransportError::IoError),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }
        fs::rename(src, dst).map_err(map_io_error)
    }

    fn delete_file(&self, path: &RelPath) -> Result<(), TransportError> {
        self.ensure_no_symlink_parents(path)?;
        let full_path = self.resolve(path);
        ensure_local_file(&full_path)?;
        fs::remove_file(full_path).map_err(map_io_error)
    }

    fn create_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.ensure_no_symlink_components(path)?;
        fs::create_dir_all(self.resolve(path)).map_err(map_io_error)
    }

    fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.ensure_no_symlink_parents(path)?;
        ensure_local_directory(&self.resolve(path))?;
        fs::remove_dir(self.resolve(path)).map_err(map_io_error)
    }

    fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError> {
        self.ensure_no_symlink_parents(path)?;
        let full_path = self.resolve(path);
        ensure_local_file_or_directory(&full_path)?;
        let system_time = parse_timestamp(&time).ok_or(TransportError::IoError)?;
        filetime::set_file_mtime(full_path, filetime::FileTime::from_system_time(system_time))
            .map_err(map_io_error)
    }
}

impl LocalBackend {
    fn resolve(&self, path: &RelPath) -> PathBuf {
        resolve_local(&self.root, path.as_str())
    }

    fn ensure_no_symlink_parents(&self, path: &RelPath) -> Result<(), TransportError> {
        ensure_no_local_symlink_components(&self.root, path.as_str(), false)
    }

    fn ensure_no_symlink_components(&self, path: &RelPath) -> Result<(), TransportError> {
        ensure_no_local_symlink_components(&self.root, path.as_str(), true)
    }
}

struct SftpBackend {
    _session: Mutex<ssh2::Session>,
    sftp: Mutex<ssh2::Sftp>,
    root: String,
}

#[derive(Debug)]
struct OpenSshBackend {
    target: String,
    port: u16,
    root: String,
}

impl TransportBackend for SftpBackend {
    fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError> {
        let directory = self.resolve(path);
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, &directory)?;
        ensure_remote_directory(&sftp, &directory)?;
        let entries = sftp.readdir(Path::new(&directory)).map_err(map_ssh_error)?;
        let mut metas = Vec::new();

        for (path, stat) in entries {
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            if let Some(meta) = entry_meta_from_sftp(name, stat) {
                metas.push(meta);
            }
        }

        Ok(metas)
    }

    fn stat(&self, path: &RelPath) -> Result<EntryMeta, TransportError> {
        let full_path = self.resolve(path);
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, &full_path)?;
        let stat = sftp.lstat(Path::new(&full_path)).map_err(map_ssh_error)?;
        entry_meta_from_sftp(basename(path), stat).ok_or(TransportError::NotFound)
    }

    fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError> {
        let full_path = self.resolve(path);
        self.ensure_remote_file(&full_path)?;
        let file = self
            .sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .open(Path::new(&full_path))
            .map_err(map_ssh_error)?;
        Ok(TransportRead::new(file))
    }

    fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError> {
        let full_path = self.resolve(path);
        if self.remote_exists(&full_path)? {
            self.ensure_remote_file(&full_path)?;
        }
        if let Some(parent) = remote_parent(&full_path) {
            self.create_remote_parents(&parent)?;
        }
        let file = self
            .sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .create(Path::new(&full_path))
            .map_err(map_ssh_error)?;
        Ok(TransportWrite::with_close(file, close_writer))
    }

    fn rename_no_overwrite(&self, src: &RelPath, dst: &RelPath) -> Result<(), TransportError> {
        let src = self.resolve(src);
        let dst = self.resolve(dst);
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, &src)?;
        ensure_remote_parent_components(&sftp, &dst)?;
        let src_stat = sftp.lstat(Path::new(&src)).map_err(map_ssh_error)?;
        if !is_sftp_file(&src_stat) && !is_sftp_directory(&src_stat) {
            return Err(TransportError::NotFound);
        }
        match sftp.lstat(Path::new(&dst)) {
            Ok(_) => return Err(TransportError::IoError),
            Err(error) => {
                let category = map_ssh_error(error);
                if category != TransportError::NotFound {
                    return Err(category);
                }
            }
        }
        sftp.rename(Path::new(&src), Path::new(&dst), None)
            .map_err(map_ssh_error)
    }

    fn delete_file(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, &full_path)?;
        ensure_remote_file(&sftp, &full_path)?;
        sftp.unlink(Path::new(&full_path)).map_err(map_ssh_error)
    }

    fn create_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.create_remote_parents(&self.resolve(path))
    }

    fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, &full_path)?;
        ensure_remote_directory(&sftp, &full_path)?;
        sftp.rmdir(Path::new(&full_path)).map_err(map_ssh_error)
    }

    fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError> {
        let seconds = parse_timestamp(&time)
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .ok_or(TransportError::IoError)?;
        let full_path = self.resolve(path);
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, &full_path)?;
        ensure_remote_file_or_directory(&sftp, &full_path)?;
        let stat = ssh2::FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: None,
            atime: None,
            mtime: Some(seconds),
        };
        sftp.setstat(Path::new(&full_path), stat)
            .map_err(map_ssh_error)
    }
}

impl SftpBackend {
    fn resolve(&self, path: &RelPath) -> String {
        join_remote_path(&self.root, path.as_str())
    }

    fn create_remote_parents(&self, path: &str) -> Result<(), TransportError> {
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        create_remote_parents(&sftp, path)
    }

    fn remote_exists(&self, path: &str) -> Result<bool, TransportError> {
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        match sftp.lstat(Path::new(path)) {
            Ok(_) => Ok(true),
            Err(error) => {
                let category = map_ssh_error(error);
                if category == TransportError::NotFound {
                    Ok(false)
                } else {
                    Err(category)
                }
            }
        }
    }

    fn ensure_remote_file(&self, path: &str) -> Result<(), TransportError> {
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        ensure_remote_parent_components(&sftp, path)?;
        ensure_remote_file(&sftp, path)
    }
}

impl TransportBackend for OpenSshBackend {
    fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError> {
        let directory = self.resolve(path);
        let script = format!(
            "dir={}\n[ -d \"$dir\" ] || exit 44\nfor entry in \"$dir\"/* \"$dir\"/.[!.]* \"$dir\"/..?*; do\n  [ -e \"$entry\" ] || continue\n  [ -L \"$entry\" ] && continue\n  name=${{entry##*/}}\n  if [ -d \"$entry\" ]; then kind=d; size=-1; elif [ -f \"$entry\" ]; then kind=f; size=$(wc -c < \"$entry\" | tr -d ' '); else continue; fi\n  mtime=$(date -u -r \"$entry\" '+%Y-%m-%d_%H-%M-%S_000000Z' 2>/dev/null || printf '1970-01-01_00-00-00_000000Z')\n  printf '%s\\t%s\\t%s\\t%s\\n' \"$name\" \"$kind\" \"$size\" \"$mtime\"\ndone\n",
            shell_quote(&directory)
        );
        let output = self.run_script(&script, None)?;
        let text = String::from_utf8(output).map_err(|_| TransportError::IoError)?;
        let mut entries = Vec::new();
        for line in text.lines() {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() != 4 {
                return Err(TransportError::IoError);
            }
            let kind = match parts[1] {
                "f" => EntryKind::File,
                "d" => EntryKind::Directory,
                _ => return Err(TransportError::IoError),
            };
            let byte_size = parts[2].parse::<i64>().map_err(|_| TransportError::IoError)?;
            entries.push(EntryMeta {
                name: parts[0].to_string(),
                kind,
                byte_size,
                mod_time: Timestamp(parts[3].to_string()),
            });
        }
        Ok(entries)
    }

    fn stat(&self, path: &RelPath) -> Result<EntryMeta, TransportError> {
        let full_path = self.resolve(path);
        let name = basename(path);
        let script = format!(
            "entry={}\n[ -L \"$entry\" ] && exit 44\nif [ -d \"$entry\" ]; then kind=d; size=-1; elif [ -f \"$entry\" ]; then kind=f; size=$(wc -c < \"$entry\" | tr -d ' '); else exit 44; fi\nmtime=$(date -u -r \"$entry\" '+%Y-%m-%d_%H-%M-%S_000000Z' 2>/dev/null || printf '1970-01-01_00-00-00_000000Z')\nprintf '%s\\t%s\\t%s\\t%s\\n' {} \"$kind\" \"$size\" \"$mtime\"\n",
            shell_quote(&full_path),
            shell_quote(&name)
        );
        let output = self.run_script(&script, None)?;
        let text = String::from_utf8(output).map_err(|_| TransportError::IoError)?;
        let parts = text.trim_end().split('\t').collect::<Vec<_>>();
        if parts.len() != 4 {
            return Err(TransportError::IoError);
        }
        let kind = match parts[1] {
            "f" => EntryKind::File,
            "d" => EntryKind::Directory,
            _ => return Err(TransportError::IoError),
        };
        Ok(EntryMeta {
            name: parts[0].to_string(),
            kind,
            byte_size: parts[2].parse::<i64>().map_err(|_| TransportError::IoError)?,
            mod_time: Timestamp(parts[3].to_string()),
        })
    }

    fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError> {
        let full_path = self.resolve(path);
        let script = format!(
            "entry={}\n[ -f \"$entry\" ] && [ ! -L \"$entry\" ] || exit 44\ncat \"$entry\"\n",
            shell_quote(&full_path)
        );
        let output = self.run_script(&script, None)?;
        Ok(TransportRead::new(io::Cursor::new(output)))
    }

    fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError> {
        let full_path = self.resolve(path);
        let mut temp_path = std::env::temp_dir();
        temp_path.push(format!(
            "kitchensync-openssh-write-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let file = File::create(&temp_path).map_err(map_io_error)?;
        let target = self.target.clone();
        let port = self.port;
        let home = home_dir();
        Ok(TransportWrite::with_close(file, move |mut writer| {
            writer.flush().map_err(map_io_error)?;
            drop(writer);
            let data = fs::read(&temp_path).map_err(map_io_error)?;
            let _ = fs::remove_file(&temp_path);
            let script = format!(
                "entry={}\nparent=${{entry%/*}}\n[ \"$parent\" = \"$entry\" ] && parent=.\nmkdir -p \"$parent\" || exit 13\nif [ -e \"$entry\" ] && {{ [ ! -f \"$entry\" ] || [ -L \"$entry\" ]; }}; then exit 44; fi\ncat > \"$entry\"\n",
                shell_quote(&full_path)
            );
            run_openssh_shell(&target, port, &script, Some(&data), home.as_deref()).map(|_| ())
        }))
    }

    fn rename_no_overwrite(&self, src: &RelPath, dst: &RelPath) -> Result<(), TransportError> {
        let src = self.resolve(src);
        let dst = self.resolve(dst);
        let script = format!(
            "src={}\ndst={}\n[ -e \"$src\" ] || exit 44\n[ ! -e \"$dst\" ] || exit 1\nmv \"$src\" \"$dst\"\n",
            shell_quote(&src),
            shell_quote(&dst)
        );
        self.run_script(&script, None).map(|_| ())
    }

    fn delete_file(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        let script = format!(
            "entry={}\n[ -f \"$entry\" ] && [ ! -L \"$entry\" ] || exit 44\nrm \"$entry\"\n",
            shell_quote(&full_path)
        );
        self.run_script(&script, None).map(|_| ())
    }

    fn create_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        let script = format!("mkdir -p {}\n", shell_quote(&full_path));
        self.run_script(&script, None).map(|_| ())
    }

    fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        let script = format!(
            "entry={}\n[ -d \"$entry\" ] && [ ! -L \"$entry\" ] || exit 44\nrmdir \"$entry\"\n",
            shell_quote(&full_path)
        );
        self.run_script(&script, None).map(|_| ())
    }

    fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        let Some(touch_time) = touch_timestamp(&time) else {
            return Err(TransportError::IoError);
        };
        let script = format!(
            "entry={}\n[ -e \"$entry\" ] || exit 44\ntouch -t {} \"$entry\"\n",
            shell_quote(&full_path),
            shell_quote(&touch_time)
        );
        self.run_script(&script, None).map(|_| ())
    }
}

impl OpenSshBackend {
    fn resolve(&self, path: &RelPath) -> String {
        join_remote_path(&self.root, path.as_str())
    }

    fn run_script(&self, script: &str, input: Option<&[u8]>) -> Result<Vec<u8>, TransportError> {
        let home = home_dir();
        run_openssh_shell(&self.target, self.port, script, input, home.as_deref())
    }
}

fn ensure_remote_file(sftp: &ssh2::Sftp, path: &str) -> Result<(), TransportError> {
    let stat = sftp.lstat(Path::new(path)).map_err(map_ssh_error)?;
    if is_sftp_file(&stat) {
        Ok(())
    } else {
        Err(TransportError::NotFound)
    }
}

fn ensure_remote_directory(sftp: &ssh2::Sftp, path: &str) -> Result<(), TransportError> {
    let stat = sftp.lstat(Path::new(path)).map_err(map_ssh_error)?;
    if is_sftp_directory(&stat) {
        Ok(())
    } else {
        Err(TransportError::NotFound)
    }
}

fn ensure_remote_file_or_directory(sftp: &ssh2::Sftp, path: &str) -> Result<(), TransportError> {
    let stat = sftp.lstat(Path::new(path)).map_err(map_ssh_error)?;
    if is_sftp_file(&stat) || is_sftp_directory(&stat) {
        Ok(())
    } else {
        Err(TransportError::NotFound)
    }
}

fn ensure_remote_parent_components(sftp: &ssh2::Sftp, path: &str) -> Result<(), TransportError> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() <= 1 {
        return Ok(());
    }

    let mut current = String::new();
    for part in parts.iter().take(parts.len() - 1) {
        current.push('/');
        current.push_str(*part);
        ensure_remote_directory(sftp, &current)?;
    }
    Ok(())
}

fn create_remote_parents(sftp: &ssh2::Sftp, path: &str) -> Result<(), TransportError> {
    let mut current = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        match sftp.lstat(Path::new(&current)) {
            Ok(stat) if is_sftp_directory(&stat) => {}
            Ok(_) => return Err(TransportError::IoError),
            Err(error) => {
                let category = map_ssh_error(error);
                if category != TransportError::NotFound {
                    return Err(category);
                }
                sftp.mkdir(Path::new(&current), 0o755)
                    .map_err(map_ssh_error)?;
            }
        }
    }
    Ok(())
}

fn connect_local(
    url: &PeerUrl,
    root_mode: TransportRootMode,
) -> Result<TransportHandle, TransportError> {
    let root = PathBuf::from(&url.path);
    match root_mode {
        TransportRootMode::RequireExisting => {
            let metadata = fs::metadata(&root).map_err(map_io_error)?;
            if !metadata.is_dir() {
                return Err(TransportError::NotFound);
            }
        }
        TransportRootMode::CreateMissing => fs::create_dir_all(&root).map_err(map_io_error)?,
    }
    Ok(TransportHandle::new(LocalBackend { root }))
}

fn connect_sftp(
    url: &PeerUrl,
    timeouts: TransportTimeouts,
    root_mode: TransportRootMode,
) -> Result<TransportHandle, TransportError> {
    let host = url.host.as_deref().ok_or(TransportError::IoError)?;
    let username = url
        .username
        .as_deref()
        .ok_or(TransportError::PermissionDenied)?;
    let port = url.port.unwrap_or(22);
    let address = (host, port)
        .to_socket_addrs()
        .map_err(map_io_error)?
        .next()
        .ok_or(TransportError::IoError)?;
    let stream = TcpStream::connect_timeout(
        &address,
        Duration::from_secs(u64::from(timeouts.timeout_conn.max(1))),
    )
    .map_err(map_io_error)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(u64::from(
            timeouts.timeout_idle.max(1),
        ))))
        .map_err(map_io_error)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(u64::from(
            timeouts.timeout_idle.max(1),
        ))))
        .map_err(map_io_error)?;

    let mut session = ssh2::Session::new().map_err(map_ssh_error)?;
    session.set_tcp_stream(stream);
    session.set_timeout(timeouts.timeout_conn.max(1) * 1000);
    prefer_compatible_kex(&session)?;
    session.handshake().map_err(map_ssh_error)?;
    verify_known_host(&session, host, port)?;
    authenticate(&session, username, url.password.as_deref())?;
    session.set_keepalive(true, timeouts.timeout_idle);

    let sftp = session.sftp().map_err(map_ssh_error)?;
    let root = normalize_remote_root(&url.path);
    match root_mode {
        TransportRootMode::RequireExisting => {
            let stat = sftp.lstat(Path::new(&root)).map_err(map_ssh_error)?;
            if !is_sftp_directory(&stat) {
                return Err(TransportError::NotFound);
            }
        }
        TransportRootMode::CreateMissing => {
            create_remote_parents(&sftp, &root)?;
        }
    }

    Ok(TransportHandle::new(SftpBackend {
        _session: Mutex::new(session),
        sftp: Mutex::new(sftp),
        root,
    }))
}

fn connect_openssh_sftp(
    url: &PeerUrl,
    root_mode: TransportRootMode,
) -> Result<TransportHandle, TransportError> {
    let host = url.host.as_deref().ok_or(TransportError::IoError)?;
    let username = url
        .username
        .as_deref()
        .ok_or(TransportError::PermissionDenied)?;
    let port = url.port.unwrap_or(22);
    let root = normalize_remote_root(&url.path);
    let target = format!("{username}@{host}");
    let backend = OpenSshBackend { target, port, root };
    match root_mode {
        TransportRootMode::RequireExisting => {
            let script = format!(
                "root={}\n[ -d \"$root\" ] || exit 44\n",
                shell_quote(&backend.root)
            );
            backend.run_script(&script, None)?;
        }
        TransportRootMode::CreateMissing => {
            let script = format!("mkdir -p {}\n", shell_quote(&backend.root));
            backend.run_script(&script, None)?;
        }
    }
    Ok(TransportHandle::new(backend))
}

fn prefer_compatible_kex(session: &ssh2::Session) -> Result<(), TransportError> {
    session
        .method_pref(
            ssh2::MethodType::Kex,
            "ecdh-sha2-nistp256,ecdh-sha2-nistp384,ecdh-sha2-nistp521,curve25519-sha256,curve25519-sha256@libssh.org",
        )
        .map_err(map_ssh_error)
}

fn authenticate(
    session: &ssh2::Session,
    username: &str,
    password: Option<&str>,
) -> Result<(), TransportError> {
    if let Some(password) = password {
        if session.userauth_password(username, password).is_ok() && session.authenticated() {
            return Ok(());
        }
    }

    if session.userauth_agent(username).is_ok() && session.authenticated() {
        return Ok(());
    }

    let home = home_dir().ok_or(TransportError::PermissionDenied)?;
    for private_key in private_key_candidates(&home) {
        if private_key.exists()
            && session
                .userauth_pubkey_file(username, None, &private_key, password)
                .is_ok()
            && session.authenticated()
        {
            return Ok(());
        }
    }

    Err(TransportError::PermissionDenied)
}

fn private_key_candidates(home: &Path) -> Vec<PathBuf> {
    let ssh_dir = home.join(".ssh");
    ["id_ed25519", "id_ecdsa", "id_rsa"]
        .into_iter()
        .map(|name| ssh_dir.join(name))
        .collect()
}

fn verify_known_host(session: &ssh2::Session, host: &str, port: u16) -> Result<(), TransportError> {
    let Some((key, _key_type)) = session.host_key() else {
        return Err(TransportError::PermissionDenied);
    };
    let mut known_hosts = session.known_hosts().map_err(map_ssh_error)?;
    if let Some(path) = home_dir().map(|home| home.join(".ssh").join("known_hosts")) {
        known_hosts
            .read_file(&path, ssh2::KnownHostFileKind::OpenSSH)
            .map_err(map_ssh_error)?;
    }
    match known_hosts.check_port(host, port, key) {
        ssh2::CheckResult::Match => Ok(()),
        _ => Err(TransportError::PermissionDenied),
    }
}

fn entry_meta_from_metadata(name: String, metadata: fs::Metadata) -> Option<EntryMeta> {
    let file_type = metadata.file_type();
    let (kind, byte_size) = if file_type.is_file() {
        (EntryKind::File, metadata.len() as i64)
    } else if file_type.is_dir() {
        (EntryKind::Directory, -1)
    } else {
        return None;
    };

    let modified = metadata.modified().ok()?;
    Some(EntryMeta {
        name,
        kind,
        mod_time: format_system_time(modified),
        byte_size,
    })
}

fn entry_meta_from_sftp(name: String, stat: ssh2::FileStat) -> Option<EntryMeta> {
    let (kind, byte_size) = if is_sftp_file(&stat) {
        (EntryKind::File, stat.size.unwrap_or(0) as i64)
    } else if is_sftp_directory(&stat) {
        (EntryKind::Directory, -1)
    } else {
        return None;
    };
    Some(EntryMeta {
        name,
        kind,
        mod_time: format_system_time(UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0))),
        byte_size,
    })
}

fn is_sftp_file(stat: &ssh2::FileStat) -> bool {
    stat.perm
        .map(|perm| (perm & SFTP_KIND_MASK) == SFTP_REGULAR_FILE)
        .unwrap_or(false)
}

fn is_sftp_directory(stat: &ssh2::FileStat) -> bool {
    stat.perm
        .map(|perm| (perm & SFTP_KIND_MASK) == SFTP_DIRECTORY)
        .unwrap_or(false)
}

fn resolve_local(root: &Path, rel_path: &str) -> PathBuf {
    if rel_path.is_empty() {
        root.to_path_buf()
    } else {
        rel_path
            .split('/')
            .fold(root.to_path_buf(), |path, segment| path.join(segment))
    }
}

fn ensure_local_file(path: &Path) -> Result<(), TransportError> {
    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if metadata.file_type().is_file() {
        Ok(())
    } else {
        Err(TransportError::NotFound)
    }
}

fn ensure_local_directory(path: &Path) -> Result<(), TransportError> {
    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(TransportError::NotFound)
    }
}

fn ensure_local_file_or_directory(path: &Path) -> Result<(), TransportError> {
    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    let file_type = metadata.file_type();
    if file_type.is_file() || file_type.is_dir() {
        Ok(())
    } else {
        Err(TransportError::NotFound)
    }
}

fn ensure_no_local_symlink_components(
    root: &Path,
    rel_path: &str,
    include_final: bool,
) -> Result<(), TransportError> {
    let parts: Vec<&str> = rel_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let limit = if include_final {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };
    let mut current = root.to_path_buf();

    for part in parts.iter().take(limit) {
        current.push(*part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(TransportError::NotFound)
            }
            Ok(_) => return Err(TransportError::IoError),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(map_io_error(error)),
        }
    }

    Ok(())
}

fn basename(path: &RelPath) -> String {
    path.as_str()
        .rsplit('/')
        .next()
        .unwrap_or(path.as_str())
        .to_string()
}

fn close_writer(mut writer: Box<dyn Write + Send>) -> Result<(), TransportError> {
    writer.flush().map_err(map_io_error)
}

fn map_io_error(error: io::Error) -> TransportError {
    match error.kind() {
        io::ErrorKind::NotFound => TransportError::NotFound,
        io::ErrorKind::PermissionDenied => TransportError::PermissionDenied,
        _ => TransportError::IoError,
    }
}

fn map_ssh_error(error: ssh2::Error) -> TransportError {
    match error.code() {
        ssh2::ErrorCode::SFTP(code) if code == SFTP_STATUS_NO_SUCH_FILE => TransportError::NotFound,
        ssh2::ErrorCode::SFTP(code) if code == SFTP_STATUS_PERMISSION_DENIED => {
            TransportError::PermissionDenied
        }
        _ => TransportError::IoError,
    }
}

fn normalize_remote_root(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn join_remote_path(root: &str, rel_path: &str) -> String {
    if rel_path.is_empty() {
        root.to_string()
    } else if root == "/" {
        format!("/{rel_path}")
    } else {
        format!("{root}/{rel_path}")
    }
}

fn remote_parent(path: &str) -> Option<String> {
    path.rsplit_once('/').and_then(|(parent, _)| {
        if parent.is_empty() {
            Some("/".to_string())
        } else {
            Some(parent.to_string())
        }
    })
}

fn shell_quote(value: &str) -> String {
    let mut quoted = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn run_openssh_shell(
    target: &str,
    port: u16,
    script: &str,
    input: Option<&[u8]>,
    home: Option<&Path>,
) -> Result<Vec<u8>, TransportError> {
    let mut command = Command::new("ssh");
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-p")
        .arg(port.to_string())
        .arg(target)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if input.is_some() {
        command.arg(script);
    } else {
        command.arg("sh").arg("-s");
    }
    if let Some(home) = home {
        command.env("HOME", home);
        command.env("USERPROFILE", home);
    }
    let mut child = command.spawn().map_err(map_io_error)?;
    {
        let stdin = child.stdin.as_mut().ok_or(TransportError::IoError)?;
        if input.is_none() {
            stdin.write_all(script.as_bytes()).map_err(map_io_error)?;
        }
        if let Some(input) = input {
            stdin.write_all(input).map_err(map_io_error)?;
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().map_err(map_io_error)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        match output.status.code() {
            Some(44) => Err(TransportError::NotFound),
            Some(13) => Err(TransportError::PermissionDenied),
            _ => Err(TransportError::IoError),
        }
    }
}

fn touch_timestamp(timestamp: &Timestamp) -> Option<String> {
    let value = timestamp.0.as_str();
    if value.len() < 19 {
        return None;
    }
    Some(format!(
        "{}{}{}{}{}.{}",
        value.get(0..4)?,
        value.get(5..7)?,
        value.get(8..10)?,
        value.get(11..13)?,
        value.get(14..16)?,
        value.get(17..19)?
    ))
}

fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        if let (Some(drive), Some(path)) = (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            let mut combined = drive.to_string_lossy().into_owned();
            combined.push_str(&path.to_string_lossy());
            let combined = PathBuf::from(combined);
            if combined.exists() {
                return Some(combined);
            }
        }
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn format_system_time(time: SystemTime) -> Timestamp {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let days = (duration.as_secs() / 86_400) as i64;
    let seconds_of_day = duration.as_secs() % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    Timestamp(format!(
        "{year:04}-{month:02}-{day:02}_{hour:02}-{minute:02}-{second:02}_{:06}Z",
        duration.subsec_micros()
    ))
}

fn parse_timestamp(timestamp: &Timestamp) -> Option<SystemTime> {
    let value = timestamp.0.as_str();
    if value.len() != 27 || !value.ends_with('Z') {
        return None;
    }
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let hour = value.get(11..13)?.parse::<u64>().ok()?;
    let minute = value.get(14..16)?.parse::<u64>().ok()?;
    let second = value.get(17..19)?.parse::<u64>().ok()?;
    let micros = value.get(20..26)?.parse::<u32>().ok()?;
    if value.get(4..5)? != "-"
        || value.get(7..8)? != "-"
        || value.get(10..11)? != "_"
        || value.get(13..14)? != "-"
        || value.get(16..17)? != "-"
        || value.get(19..20)? != "_"
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
        || micros > 999_999
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let seconds = days as u64 * 86_400 + hour * 3_600 + minute * 60 + second.min(59);
    Some(UNIX_EPOCH + Duration::new(seconds, micros * 1_000))
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > max_day {
        return None;
    }
    let year = year as i64 - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_key_candidates_follow_specified_auth_order() {
        let home = Path::new("/home/example");
        let candidates = private_key_candidates(home)
            .into_iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();

        assert_eq!(
            candidates,
            vec![
                "/home/example/.ssh/id_ed25519",
                "/home/example/.ssh/id_ecdsa",
                "/home/example/.ssh/id_rsa",
            ]
        );
    }

}
