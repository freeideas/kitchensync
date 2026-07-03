use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_root() -> Result<PathBuf, Box<dyn Error>> {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kitchensync-local-filesystem-{}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = temp_root()?;

    let file_path = root.join("mtime.txt");
    fs::write(&file_path, b"mtime")?;
    let wanted = UNIX_EPOCH + Duration::from_secs(1_704_110_400) + Duration::from_micros(123_456);
    let file = OpenOptions::new().write(true).open(&file_path)?;
    file.set_modified(wanted)?;
    let observed = fs::metadata(&file_path)?.modified()?;
    assert_eq!(
        observed.duration_since(UNIX_EPOCH)?.as_micros(),
        wanted.duration_since(UNIX_EPOCH)?.as_micros()
    );

    let original_dir = root.join("dir");
    fs::create_dir_all(original_dir.join("nested"))?;
    fs::write(original_dir.join("nested").join("file.txt"), b"kept")?;
    let moved_dir = root.join("moved");
    fs::rename(&original_dir, &moved_dir)?;
    assert!(!original_dir.exists());
    assert_eq!(fs::read(moved_dir.join("nested").join("file.txt"))?, b"kept");

    let src = root.join("src.txt");
    let dst = root.join("dst.txt");
    {
        let mut f = File::create(&src)?;
        f.write_all(b"source")?;
    }
    fs::write(&dst, b"dest")?;
    fs::rename(&src, &dst)?;
    assert!(!src.exists());
    assert_eq!(fs::read(&dst)?, b"source");

    fs::remove_dir_all(root)?;
    println!("checked local mtime setting, directory rename, and Linux rename-over-existing behavior");
    Ok(())
}
