use std::sync::Arc;

use crate::database::Database;

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum LogLevel {
    Error = 0,
    Info = 1,
    Debug = 2,
    Trace = 3,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Self {
        match s {
            "error" => LogLevel::Error,
            "info" => LogLevel::Info,
            "debug" => LogLevel::Debug,
            "trace" => LogLevel::Trace,
            _ => LogLevel::Info,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

pub struct Logger {
    db: Arc<Database>,
    level: LogLevel,
    log_retention_days: u64,
}

impl Logger {
    pub fn new(db: Arc<Database>, log_retention_days: u64) -> Self {
        // Read or initialize log level
        let level = match db.get_config("log-level") {
            Some(l) => LogLevel::from_str(&l),
            None => {
                db.set_config("log-level", "info");
                LogLevel::Info
            }
        };
        Logger {
            db,
            level,
            log_retention_days,
        }
    }

    pub fn log(&self, level: LogLevel, message: &str) {
        if level > self.level {
            return;
        }
        self.db.log(level.as_str(), message, self.log_retention_days);

        // KitchenSync: info and error also go to stdout
        if level == LogLevel::Info || level == LogLevel::Error {
            println!("{}", message);
        }
    }

    pub fn error(&self, message: &str) {
        self.log(LogLevel::Error, message);
    }

    pub fn info(&self, message: &str) {
        self.log(LogLevel::Info, message);
    }

    pub fn debug(&self, message: &str) {
        self.log(LogLevel::Debug, message);
    }

    pub fn trace(&self, message: &str) {
        self.log(LogLevel::Trace, message);
    }
}
