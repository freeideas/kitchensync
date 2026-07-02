use filetime::{set_file_mtime, FileTime};
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "kitchensync-local-file-metadata-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let file_path = root.join("file.txt");
    fs::write(&file_path, b"abc")?;
    let dir_path = root.join("dir");
    fs::create_dir(&dir_path)?;

    let file_time = FileTime::from_unix_time(1_700_000_001, 123_456_000);
    let dir_time = FileTime::from_unix_time(1_700_000_002, 654_321_000);
    set_file_mtime(&file_path, file_time)?;
    set_file_mtime(&dir_path, dir_time)?;

    let read_file = FileTime::from_last_modification_time(&fs::metadata(&file_path)?);
    let read_dir = FileTime::from_last_modification_time(&fs::metadata(&dir_path)?);
    assert_eq!(read_file.unix_seconds(), 1_700_000_001);
    assert_eq!(read_file.nanoseconds() / 1_000, 123_456);
    assert_eq!(read_dir.unix_seconds(), 1_700_000_002);
    assert_eq!(read_dir.nanoseconds() / 1_000, 654_321);

    fs::remove_dir_all(root)?;
    println!("checked filetime set_file_mtime for files and directories with microsecond precision");
    Ok(())
}

