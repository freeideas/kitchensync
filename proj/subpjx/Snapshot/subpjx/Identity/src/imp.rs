use std::sync::Arc;
use crate::api::*;

struct IdentityImpl;

const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn canonicalize(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

fn hash_and_encode(canonical: &str) -> String {
    use twox_hash::XxHash64;
    use std::hash::Hasher;
    let mut hasher = XxHash64::with_seed(0);
    hasher.write(canonical.as_bytes());
    let mut n = hasher.finish();
    let mut buf = [b'0'; 11];
    for i in (0..11).rev() {
        buf[i] = ALPHABET[(n % 62) as usize];
        n /= 62;
    }
    String::from_utf8(buf.to_vec()).unwrap()
}

impl Identity for IdentityImpl {
    fn identity(&self, path: &str) -> String {
        if path == "/" {
            hash_and_encode("/")
        } else {
            hash_and_encode(&canonicalize(path))
        }
    }

    fn parent_identity(&self, path: &str) -> String {
        let canonical = canonicalize(path);
        let parent = match canonical.rfind('/') {
            Some(pos) => canonical[..pos].to_string(),
            None => "/".to_string(),
        };
        hash_and_encode(&parent)
    }
}

pub fn new() -> std::sync::Arc<dyn Identity> {
    Arc::new(IdentityImpl)
}
