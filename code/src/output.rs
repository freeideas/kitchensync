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
/// When the last line was printed, for the "still scanning" heartbeat.
static LAST_LINE: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Seconds of silence after which the walk announces where it is.
pub const QUIET_SECS: u64 = 30;

/// True when nothing has been printed for `QUIET_SECS`; the caller then
/// prints an `S <relpath>` line, which resets the clock.
pub fn quiet_for_a_while() -> bool {
    LAST_LINE.lock().unwrap().is_some_and(|t| t.elapsed().as_secs() >= QUIET_SECS)
}

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
    *LAST_LINE.lock().unwrap() = Some(std::time::Instant::now());
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
