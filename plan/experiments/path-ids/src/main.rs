use xxhash_rust::xxh64::{xxh64, Xxh64};

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn base62_11(mut value: u64) -> String {
    let mut out = [b'0'; 11];
    for slot in out.iter_mut().rev() {
        *slot = BASE62[(value % 62) as usize];
        value /= 62;
    }
    assert_eq!(value, 0);
    String::from_utf8(out.to_vec()).expect("base62 ascii")
}

fn id(path: &str) -> String {
    base62_11(xxh64(path.as_bytes(), 0))
}

fn streaming_hash(path: &str) -> u64 {
    let mut hasher = Xxh64::new(0);
    let bytes = path.as_bytes();
    let split = bytes.len().min(4);
    hasher.update(&bytes[..split]);
    hasher.update(&bytes[split..]);
    hasher.digest()
}

fn main() {
    let cases = [
        ("/", "JyBskcNRrBK"),
        ("docs", "H41WPg3SlMv"),
        ("docs/readme.txt", "K5EzsWuLZ04"),
        ("docs/notes", "1pP6ATZM5gH"),
    ];
    for (path, placeholder) in cases {
        let one_shot = xxh64(path.as_bytes(), 0);
        assert_eq!(one_shot, streaming_hash(path));
        let encoded = id(path);
        assert_eq!(encoded.len(), 11);
        assert!(encoded.bytes().all(|b| BASE62.contains(&b)));
        println!("{} {}", path, encoded);
        assert_eq!(encoded, placeholder);
    }
    assert_eq!(id("docs/readme.txt"), id("docs/readme.txt"));
    assert_ne!(id("docs/readme.txt"), id("docs/notes"));
    println!("checked xxhash-rust xxh64 seed 0 and 11-character base62 encoding");
}
