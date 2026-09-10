//! All program output goes to stdout, in order, filtered by verbosity.
//! See specs/sync.md, section "Logging".

use std::io::Write;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Error = 0,
    Info = 1,
    Debug = 2,
    Trace = 3,
}

impl Verbosity {
    pub fn parse(s: &str) -> Option<Verbosity> {
        match s {
            "error" => Some(Verbosity::Error),
            "info" => Some(Verbosity::Info),
            "debug" => Some(Verbosity::Debug),
            "trace" => Some(Verbosity::Trace),
            _ => None,
        }
    }
}

static LEVEL: Mutex<Verbosity> = Mutex::new(Verbosity::Info);
static OUT: Mutex<()> = Mutex::new(());

pub fn set_level(v: Verbosity) {
    *LEVEL.lock().unwrap() = v;
}

pub fn level() -> Verbosity {
    *LEVEL.lock().unwrap()
}

/// Print one line to stdout unconditionally (help text, completion line, errors).
pub fn line(s: &str) {
    let _g = OUT.lock().unwrap();
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.write_all(b"\n");
    let _ = o.flush();
}

/// Print raw text without adding a newline.
pub fn raw(s: &str) {
    let _g = OUT.lock().unwrap();
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

pub fn error(s: &str) {
    line(s);
}

pub fn info(s: &str) {
    if level() >= Verbosity::Info {
        line(s);
    }
}

pub fn trace(s: &str) {
    if level() >= Verbosity::Trace {
        line(s);
    }
}
