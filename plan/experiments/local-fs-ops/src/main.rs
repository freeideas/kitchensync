use std::error::Error;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("kitchensync-local-fs-ops-{}", std::process::id()))
}

fn rename_to_missing(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    if dst.try_exists()? {
        return Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            "rename destination already exists",
        ));
    }
    fs::rename(src, dst)
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = temp_root();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;

    let nested = root.join("a/b");
    fs::create_dir_all(&nested)?;
    let source = nested.join("source.txt");
    {
        let mut writer = File::create(&source)?;
        writer.write_all(b"bounded local write")?;
        writer.flush()?;
    }

    let mut listing: Vec<(String, bool, i64)> = fs::read_dir(root.join("a"))?
        .map(|entry| {
            let entry = entry.expect("read_dir entry");
            let metadata = entry.metadata().expect("metadata");
            let file_type = entry.file_type().expect("file_type");
            let byte_size = if file_type.is_dir() {
                -1
            } else {
                metadata.len() as i64
            };
            (
                entry.file_name().to_string_lossy().to_string(),
                file_type.is_dir(),
                byte_size,
            )
        })
        .collect();
    listing.sort();
    assert_eq!(listing, vec![("b".to_string(), true, -1)]);

    let moved = nested.join("moved.txt");
    rename_to_missing(&source, &moved)?;
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(&moved)?, "bounded local write");

    let existing = nested.join("existing.txt");
    fs::write(&existing, b"keep me")?;
    let err = rename_to_missing(&moved, &existing).expect_err("destination must be rejected");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&moved)?, "bounded local write");
    assert_eq!(fs::read_to_string(&existing)?, "keep me");

    let bak_parent = root.join("a/.kitchensync/BAK/2026-07-02_00-00-00_000001Z");
    fs::create_dir_all(&bak_parent)?;
    let displaced_dir = bak_parent.join("b");
    rename_to_missing(&nested, &displaced_dir)?;
    assert!(!nested.exists());
    assert_eq!(
        fs::read_to_string(displaced_dir.join("moved.txt"))?,
        "bounded local write"
    );

    let mut reader = File::open(displaced_dir.join("existing.txt"))?;
    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;
    assert_eq!(contents, "keep me");

    fs::remove_file(displaced_dir.join("moved.txt"))?;
    fs::remove_file(displaced_dir.join("existing.txt"))?;
    fs::remove_dir(&displaced_dir)?;
    assert!(!displaced_dir.exists());

    fs::remove_dir_all(root)?;
    println!("checked std fs create, list, stream, no-overwrite rename, directory rename, and delete calls");
    Ok(())
}
