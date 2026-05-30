use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
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
            "sftp" => connect_sftp(url, timeouts, root_mode),
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
        let full_path = self.resolve(path);
        let metadata = fs::symlink_metadata(&full_path).map_err(map_io_error)?;
        entry_meta_from_metadata(basename(path), metadata).ok_or(TransportError::NotFound)
    }

    fn open_read(&self, path: &RelPath) -> Result<TransportRead, TransportError> {
        let full_path = self.resolve(path);
        ensure_local_file(&full_path)?;
        let file = File::open(full_path).map_err(map_io_error)?;
        Ok(TransportRead::new(file))
    }

    fn open_write(&self, path: &RelPath) -> Result<TransportWrite, TransportError> {
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
        let src = self.resolve(src);
        let dst = self.resolve(dst);
        match fs::symlink_metadata(&dst) {
            Ok(_) => return Err(TransportError::IoError),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }
        fs::rename(src, dst).map_err(map_io_error)
    }

    fn delete_file(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        ensure_local_file(&full_path)?;
        fs::remove_file(full_path).map_err(map_io_error)
    }

    fn create_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        fs::create_dir_all(self.resolve(path)).map_err(map_io_error)
    }

    fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        fs::remove_dir(self.resolve(path)).map_err(map_io_error)
    }

    fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError> {
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
}

struct SftpBackend {
    _session: Mutex<ssh2::Session>,
    sftp: Mutex<ssh2::Sftp>,
    root: String,
}

impl TransportBackend for SftpBackend {
    fn list_dir(&self, path: &RelPath) -> Result<Vec<EntryMeta>, TransportError> {
        let directory = self.resolve(path);
        let entries = self
            .sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .readdir(Path::new(&directory))
            .map_err(map_ssh_error)?;
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
        let stat = self
            .sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .lstat(Path::new(&self.resolve(path)))
            .map_err(map_ssh_error)?;
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
        if sftp.lstat(Path::new(&dst)).is_ok() {
            return Err(TransportError::IoError);
        }
        sftp.rename(Path::new(&src), Path::new(&dst), None)
            .map_err(map_ssh_error)
    }

    fn delete_file(&self, path: &RelPath) -> Result<(), TransportError> {
        let full_path = self.resolve(path);
        self.ensure_remote_file(&full_path)?;
        self.sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .unlink(Path::new(&full_path))
            .map_err(map_ssh_error)
    }

    fn create_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.create_remote_parents(&self.resolve(path))
    }

    fn delete_dir(&self, path: &RelPath) -> Result<(), TransportError> {
        self.sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .rmdir(Path::new(&self.resolve(path)))
            .map_err(map_ssh_error)
    }

    fn set_mod_time(&self, path: &RelPath, time: Timestamp) -> Result<(), TransportError> {
        let seconds = parse_timestamp(&time)
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .ok_or(TransportError::IoError)?;
        let full_path = self.resolve(path);
        self.ensure_remote_file_or_directory(&full_path)?;
        let stat = ssh2::FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: None,
            atime: None,
            mtime: Some(seconds),
        };
        self.sftp
            .lock()
            .map_err(|_| TransportError::IoError)?
            .setstat(Path::new(&full_path), stat)
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
        let stat = sftp.lstat(Path::new(path)).map_err(map_ssh_error)?;
        if is_sftp_file(&stat) {
            Ok(())
        } else {
            Err(TransportError::NotFound)
        }
    }

    fn ensure_remote_file_or_directory(&self, path: &str) -> Result<(), TransportError> {
        let sftp = self.sftp.lock().map_err(|_| TransportError::IoError)?;
        let stat = sftp.lstat(Path::new(path)).map_err(map_ssh_error)?;
        if is_sftp_file(&stat) || is_sftp_directory(&stat) {
            Ok(())
        } else {
            Err(TransportError::NotFound)
        }
    }
}

fn create_remote_parents(sftp: &ssh2::Sftp, path: &str) -> Result<(), TransportError> {
    let mut current = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        match sftp.lstat(Path::new(&current)) {
            Ok(stat) if is_sftp_directory(&stat) => {}
            Ok(_) => return Err(TransportError::IoError),
            Err(_) => {
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
    let private_key = home.join(".ssh").join("id_rsa");
    if private_key.exists()
        && session
            .userauth_pubkey_file(username, None, &private_key, password)
            .is_ok()
        && session.authenticated()
    {
        return Ok(());
    }

    Err(TransportError::PermissionDenied)
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

fn home_dir() -> Option<PathBuf> {
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
