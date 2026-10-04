//! Unicode normalization of names. The same visible name can be stored as
//! different bytes: macOS reports "é" decomposed (e + combining accent, NFD)
//! while Linux and Windows usually keep it composed (one character, NFC).
//! Compared byte for byte, such a pair looks like two different files, so a
//! sync between them displaces one and copies the other back every run.
//!
//! `Normalizing` wraps a peer's transport so the engine only ever sees NFC
//! names. It remembers the name each entry really has on disk, from the
//! listings, and turns the engine's NFC paths back into those real paths, so
//! nothing is renamed on disk. See specs/sync.md, "Unicode Normalization".

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use unicode_normalization::{is_nfc, UnicodeNormalization};

use super::{join, Entry, ReadHandle, Result, Transport, WriteHandle};

pub fn nfc(s: &str) -> String {
    if is_nfc(s) { s.to_string() } else { s.nfc().collect() }
}

pub struct Normalizing {
    inner: Arc<dyn Transport>,
    /// NFC path -> real path, only for entries whose real name is not NFC.
    real: Mutex<HashMap<String, String>>,
}

impl Normalizing {
    pub fn new(inner: Arc<dyn Transport>) -> Normalizing {
        Normalizing { inner, real: Mutex::new(HashMap::new()) }
    }

    /// The real path for an NFC path: each component is looked up in the
    /// names learned from listings, and kept as given when none was learned.
    fn resolve(&self, path: &str) -> String {
        let map = self.real.lock().unwrap();
        if map.is_empty() || path.is_ascii() {
            return path.to_string();
        }
        let mut key = String::new();
        let mut real = String::new();
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            key = join(&key, comp);
            real = match map.get(&key) {
                Some(r) => r.clone(),
                None => join(&real, comp),
            };
        }
        real
    }

    fn forget(&self, path: &str) {
        let mut map = self.real.lock().unwrap();
        if !map.is_empty() {
            map.remove(path);
        }
    }
}

impl Transport for Normalizing {
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>> {
        let real_dir = self.resolve(path);
        let entries = self.inner.list_dir(&real_dir)?;
        if entries.iter().all(|e| e.name.is_ascii()) {
            return Ok(entries);
        }
        let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
        let mut map = self.real.lock().unwrap();
        for mut e in entries {
            let n = nfc(&e.name);
            let key = join(path, &n);
            if let Some(other) = out.iter().find(|o| o.name == n) {
                // Two names on disk that differ only in normalization: only a
                // filesystem that compares bytes can hold both. Sync the
                // composed one and leave the other alone.
                crate::output::error(&format!(
                    "skipping {}: another entry in the same directory has the same name in a different Unicode form ({})",
                    join(&real_dir, &e.name),
                    other.name
                ));
                if e.name == n {
                    map.remove(&key);
                    out.retain(|o| o.name != n);
                    out.push(e);
                }
                continue;
            }
            if n != e.name {
                map.insert(key, join(&real_dir, &e.name));
                e.name = n;
            } else {
                map.remove(&key);
            }
            out.push(e);
        }
        Ok(out)
    }

    fn stat(&self, path: &str) -> Result<Entry> {
        let mut e = self.inner.stat(&self.resolve(path))?;
        e.name = nfc(&e.name);
        Ok(e)
    }

    fn open_read(&self, path: &str) -> Result<Box<dyn ReadHandle>> {
        self.inner.open_read(&self.resolve(path))
    }

    fn open_write(&self, path: &str) -> Result<Box<dyn WriteHandle>> {
        self.inner.open_write(&self.resolve(path))
    }

    fn rename(&self, src: &str, dst: &str) -> Result<()> {
        self.inner.rename(&self.resolve(src), &self.resolve(dst))?;
        self.forget(src);
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<()> {
        self.inner.delete_file(&self.resolve(path))?;
        self.forget(path);
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<()> {
        self.inner.create_dir(&self.resolve(path))
    }

    fn delete_dir(&self, path: &str) -> Result<()> {
        self.inner.delete_dir(&self.resolve(path))?;
        self.forget(path);
        Ok(())
    }

    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<()> {
        self.inner.set_mod_time(&self.resolve(path), time)
    }

    fn preload(&self) {
        self.inner.preload()
    }
}
