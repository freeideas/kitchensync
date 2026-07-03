use std::error::Error;
use std::hash::Hasher;
use twox_hash::XxHash64;

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn xxh64_seed0(text: &str) -> u64 {
    let mut hasher = XxHash64::with_seed(0);
    hasher.write(text.as_bytes());
    hasher.finish()
}

fn base62_11(mut value: u64) -> String {
    let mut out = [b'0'; 11];
    for slot in out.iter_mut().rev() {
        *slot = DIGITS[(value % 62) as usize];
        value /= 62;
    }
    assert_eq!(value, 0, "u64 should fit in 11 base62 characters");
    String::from_utf8(out.to_vec()).unwrap()
}

fn main() -> Result<(), Box<dyn Error>> {
    assert_eq!(xxh64_seed0(""), 0xef46_db37_51d8_e999);

    let docs_readme = base62_11(xxh64_seed0("docs/readme.txt"));
    let docs_notes = base62_11(xxh64_seed0("docs/notes"));
    let docs_parent = base62_11(xxh64_seed0("docs"));
    let root_parent = base62_11(xxh64_seed0("/"));

    assert_eq!(docs_readme, "K5EzsWuLZ04");
    assert_eq!(docs_notes, "1pP6ATZM5gH");
    assert_eq!(docs_parent, "H41WPg3SlMv");
    assert_eq!(root_parent, "JyBskcNRrBK");

    println!("docs/readme.txt {docs_readme}");
    println!("docs/notes {docs_notes}");
    println!("parent docs {docs_parent}");
    println!("root sentinel {root_parent}");
    Ok(())
}
