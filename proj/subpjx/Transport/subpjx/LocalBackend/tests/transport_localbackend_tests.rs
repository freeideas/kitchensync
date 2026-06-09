use std::fs;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};
use transport_localbackend::{new, LocalBackend, LocalError};

fn setup_dir(name: &str) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("lb_test_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let url = format!("file://{}", dir.display());
    (dir, url)
}

fn nonexistent_path(name: &str) -> (PathBuf, String) {
    let path = std::env::temp_dir().join(format!("lb_test_{}", name));
    let _ = fs::remove_dir_all(&path);
    let _ = fs::remove_file(&path);
    let url = format!("file://{}", path.display());
    (path, url)
}

// 005.9, 005.11: normal run creates missing root and any missing parent directories
#[test]
fn open_root_normal_run_creates_missing_root_and_parents() {
    let (base, _) = nonexistent_path("open_root_creates");
    let deep = base.join("sub1").join("sub2");
    let url = format!("file://{}", deep.display());
    let backend = new();
    assert!(backend.open_root(&url, false).is_ok());
    assert!(deep.is_dir());
}

// 005.12: normal run treats a root that cannot be created as failed
#[test]
fn open_root_normal_run_fails_when_root_cannot_be_created() {
    let (dir, _) = setup_dir("open_root_uncreatable");
    let blocker = dir.join("blocker");
    fs::write(&blocker, b"").unwrap();
    // A file sits where a parent directory would need to exist.
    let url = format!("file://{}/child", blocker.display());
    let backend = new();
    assert!(backend.open_root(&url, false).is_err());
}

// 005.13, 005.14, 024.11: dry-run does not create the missing root and treats it as failed
#[test]
fn open_root_dry_run_does_not_create_missing_root() {
    let (path, url) = nonexistent_path("open_root_dryrun");
    let backend = new();
    assert!(backend.open_root(&url, true).is_err());
    assert!(!path.exists(), "dry-run must not create the missing root directory");
}

// dry-run positive: succeeds when the root directory already exists
#[test]
fn open_root_dry_run_succeeds_when_root_already_exists() {
    let (_, url) = setup_dir("open_root_dryrun_exists");
    let backend = new();
    assert!(backend.open_root(&url, true).is_ok());
}

// 022.2, 022.3: list_dir returns name, is_dir, mod_time, and byte_size; byte_size is file size
#[test]
fn list_dir_returns_entry_fields_for_regular_file() {
    let (dir, url) = setup_dir("list_dir_file_fields");
    fs::write(dir.join("data.txt"), b"hello world").unwrap();
    let backend = new();
    let entries = backend.list_dir(&url, "").unwrap();
    let entry = entries.iter().find(|e| e.name == "data.txt").expect("file entry missing");
    assert!(!entry.is_dir);
    assert_eq!(entry.byte_size, 11);
    // mod_time must be readable (no specific value required)
    let _ = entry.mod_time;
}

// 022.4: list_dir reports byte_size as -1 for a directory
#[test]
fn list_dir_byte_size_is_negative_one_for_directory() {
    let (dir, url) = setup_dir("list_dir_dir_size");
    fs::create_dir(dir.join("adir")).unwrap();
    let backend = new();
    let entries = backend.list_dir(&url, "").unwrap();
    let entry = entries.iter().find(|e| e.name == "adir").expect("dir entry missing");
    assert!(entry.is_dir);
    assert_eq!(entry.byte_size, -1);
}

// 022.5: stat returns mod_time, byte_size, and is_dir for an existing regular file
#[test]
fn stat_returns_metadata_for_regular_file() {
    let (dir, url) = setup_dir("stat_file");
    fs::write(dir.join("file.txt"), b"hello").unwrap();
    let backend = new();
    let s = backend.stat(&url, "file.txt").unwrap();
    assert!(!s.is_dir);
    assert_eq!(s.byte_size, 5);
    let _ = s.mod_time;
}

// 022.5: stat returns metadata for an existing directory
#[test]
fn stat_returns_metadata_for_directory() {
    let (dir, url) = setup_dir("stat_dir");
    fs::create_dir(dir.join("mydir")).unwrap();
    let backend = new();
    let s = backend.stat(&url, "mydir").unwrap();
    assert!(s.is_dir);
    assert_eq!(s.byte_size, -1);
}

// 022.6, 022.17: stat returns NotFound when the path does not exist
#[test]
fn stat_returns_not_found_for_missing_path() {
    let (_, url) = setup_dir("stat_notfound");
    let backend = new();
    let result = backend.stat(&url, "no_such.txt");
    assert!(matches!(result, Err(LocalError::NotFound)));
}

// 022.7: read returns the next chunk of bytes and None at end of file
#[test]
fn read_returns_chunks_and_none_at_end_of_file() {
    let content = b"abcdefghij";
    let (dir, url) = setup_dir("read_eof");
    fs::write(dir.join("data.bin"), content).unwrap();
    let backend = new();
    let handle = backend.open_read(&url, "data.bin").unwrap();
    let mut collected: Vec<u8> = Vec::new();
    loop {
        match backend.read(&handle, 4).unwrap() {
            Some(chunk) => collected.extend_from_slice(&chunk),
            None => break,
        }
    }
    backend.close_read(handle).unwrap();
    assert_eq!(collected.as_slice(), content.as_ref());
}

// 022.8: open_write creates the target file and any missing parent directories
#[test]
fn open_write_creates_file_and_missing_parent_directories() {
    let (_, url) = setup_dir("open_write");
    let backend = new();
    let wh = backend.open_write(&url, "a/b/out.txt").unwrap();
    backend.write(&wh, b"written").unwrap();
    backend.close_write(wh).unwrap();
    let rh = backend.open_read(&url, "a/b/out.txt").unwrap();
    let chunk = backend.read(&rh, 100).unwrap().expect("expected data after write");
    backend.close_read(rh).unwrap();
    assert_eq!(chunk, b"written");
}

// 022.9: create_dir creates the directory and any missing parent directories
#[test]
fn create_dir_creates_directory_and_missing_parents() {
    let (dir, url) = setup_dir("create_dir");
    let backend = new();
    backend.create_dir(&url, "x/y/z").unwrap();
    assert!(dir.join("x").join("y").join("z").is_dir());
}

// 022.10: rename moves src to dst when dst does not exist
#[test]
fn rename_moves_src_to_dst_when_dst_does_not_exist() {
    let (dir, url) = setup_dir("rename_ok");
    fs::write(dir.join("src.txt"), b"data").unwrap();
    let backend = new();
    backend.rename(&url, "src.txt", "dst.txt").unwrap();
    assert!(!dir.join("src.txt").exists(), "src must be gone after rename");
    assert!(dir.join("dst.txt").exists(), "dst must exist after rename");
}

// 022.11: rename fails when dst already exists
#[test]
fn rename_fails_when_dst_already_exists() {
    let (dir, url) = setup_dir("rename_fail");
    fs::write(dir.join("src.txt"), b"source").unwrap();
    fs::write(dir.join("dst.txt"), b"existing").unwrap();
    let backend = new();
    assert!(backend.rename(&url, "src.txt", "dst.txt").is_err());
}

// 022.12: delete_file removes a file
#[test]
fn delete_file_removes_a_file() {
    let (dir, url) = setup_dir("delete_file");
    fs::write(dir.join("bye.txt"), b"bye").unwrap();
    let backend = new();
    backend.delete_file(&url, "bye.txt").unwrap();
    assert!(!dir.join("bye.txt").exists());
}

// 022.13: delete_dir removes an empty directory
#[test]
fn delete_dir_removes_an_empty_directory() {
    let (dir, url) = setup_dir("delete_dir");
    fs::create_dir(dir.join("emptydir")).unwrap();
    let backend = new();
    backend.delete_dir(&url, "emptydir").unwrap();
    assert!(!dir.join("emptydir").exists());
}

// 022.14: set_mod_time sets the modification time of a file
#[test]
fn set_mod_time_sets_modification_time_of_a_file() {
    let (dir, url) = setup_dir("set_mod_time_file");
    fs::write(dir.join("f.txt"), b"x").unwrap();
    let target = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let backend = new();
    backend.set_mod_time(&url, "f.txt", target).unwrap();
    let s = backend.stat(&url, "f.txt").unwrap();
    let diff = if s.mod_time >= target {
        s.mod_time.duration_since(target).unwrap()
    } else {
        target.duration_since(s.mod_time).unwrap()
    };
    assert!(diff <= Duration::from_secs(2), "mod_time should be within 2s of target");
}

// 022.14: set_mod_time sets the modification time of a directory
#[test]
fn set_mod_time_sets_modification_time_of_a_directory() {
    let (dir, url) = setup_dir("set_mod_time_dir");
    fs::create_dir(dir.join("timed_dir")).unwrap();
    let target = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let backend = new();
    backend.set_mod_time(&url, "timed_dir", target).unwrap();
    let s = backend.stat(&url, "timed_dir").unwrap();
    let diff = if s.mod_time >= target {
        s.mod_time.duration_since(target).unwrap()
    } else {
        target.duration_since(s.mod_time).unwrap()
    };
    assert!(diff <= Duration::from_secs(2), "mod_time should be within 2s of target");
}

// 022.15: list_dir includes regular files and directories (symlink creation excluded per
// testing guidelines; only the positive inclusion side is asserted here)
#[test]
fn list_dir_includes_regular_files_and_directories() {
    let (dir, url) = setup_dir("list_dir_regular");
    fs::write(dir.join("reg.txt"), b"").unwrap();
    fs::create_dir(dir.join("reg_dir")).unwrap();
    let backend = new();
    let entries = backend.list_dir(&url, "").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"reg.txt"), "regular file must appear in listing");
    assert!(names.contains(&"reg_dir"), "directory must appear in listing");
}
