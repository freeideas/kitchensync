use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::api::Clock;

struct ClockImpl {
    last_micros: Mutex<u64>,
}

fn format_micros(us: u64) -> String {
    let total_secs = (us / 1_000_000) as i64;
    let frac_us = (us % 1_000_000) as u32;

    let sec_of_day = (total_secs % 86_400) as u32;
    let day = total_secs / 86_400;

    let h = sec_of_day / 3_600;
    let m = (sec_of_day % 3_600) / 60;
    let s = sec_of_day % 60;

    // Fliegel-Van Flandern: Julian day number to Gregorian (y, mo, d).
    // Julian day for 1970-01-01 = 2440588.
    let jd = day + 2_440_588i64;
    let l = jd + 68_569;
    let n = (4 * l) / 146_097;
    let l = l - (146_097 * n + 3) / 4;
    let i = (4_000 * (l + 1)) / 1_461_001;
    let l = l - (1_461 * i) / 4 + 31;
    let j = (80 * l) / 2_447;
    let d = l - (2_447 * j) / 80;
    let l = j / 11;
    let mo = j + 2 - 12 * l;
    let y = 100 * (n - 49) + i + l;

    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{:06}Z",
        y, mo, d, h, m, s, frac_us
    )
}

impl Clock for ClockImpl {
    fn now(&self) -> String {
        let mut last = self.last_micros.lock().unwrap();
        let current = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_micros() as u64;
        let chosen = if current > *last { current } else { *last + 1 };
        *last = chosen;
        format_micros(chosen)
    }
}

pub fn new() -> Arc<dyn Clock> {
    Arc::new(ClockImpl {
        last_micros: Mutex::new(0),
    })
}
