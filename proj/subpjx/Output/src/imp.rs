use std::sync::{Arc, Mutex};
use crate::api::*;

fn verbosity_rank(v: &Verbosity) -> u8 {
    match v {
        Verbosity::Error => 0,
        Verbosity::Info  => 1,
        Verbosity::Debug => 1, // observationally identical to Info (023.13)
        Verbosity::Trace => 2,
    }
}

fn phase_str(phase: &FailedPhase) -> &'static str {
    match phase {
        FailedPhase::ReadSource           => "read_source",
        FailedPhase::WriteSwapNew         => "write_swap_new",
        FailedPhase::MoveExistingToSwapOld => "move_existing_to_swap_old",
        FailedPhase::RenameFinal          => "rename_final",
        FailedPhase::SetModTime           => "set_mod_time",
        FailedPhase::ArchiveOld           => "archive_old",
        FailedPhase::Cleanup              => "cleanup",
    }
}

struct OutputImpl {
    verbosity: Mutex<Verbosity>,
}

impl OutputImpl {
    fn at_or_above(&self, threshold: &Verbosity) -> bool {
        let v = self.verbosity.lock().unwrap();
        verbosity_rank(&*v) >= verbosity_rank(threshold)
    }
}

impl Output for OutputImpl {
    fn set_verbosity(&self, level: Verbosity) {
        *self.verbosity.lock().unwrap() = level;
    }

    fn copied(&self, relpath: &str) {
        if self.at_or_above(&Verbosity::Info) {
            println!("C {}", relpath);
        }
    }

    fn displaced(&self, relpath: &str) {
        if self.at_or_above(&Verbosity::Info) {
            println!("X {}", relpath);
        }
    }

    fn diagnostic(&self, message: &str) {
        // Error is the minimum level; always emits.
        if self.at_or_above(&Verbosity::Error) {
            println!("{}", message);
        }
    }

    fn transfer_failed(
        &self,
        relpath: &str,
        peer_url: &str,
        phase: FailedPhase,
        error_category: Option<&str>,
    ) {
        if self.at_or_above(&Verbosity::Error) {
            match error_category {
                Some(cat) => println!(
                    "transfer failed: {} -> {} phase={} error={}",
                    relpath, peer_url, phase_str(&phase), cat
                ),
                None => println!(
                    "transfer failed: {} -> {} phase={}",
                    relpath, peer_url, phase_str(&phase)
                ),
            }
        }
    }

    fn copy_slots(&self, active: usize, max: usize) {
        if self.at_or_above(&Verbosity::Trace) {
            println!("copy-slots active={}/{}", active, max);
        }
    }
}

pub fn new() -> std::sync::Arc<dyn Output> {
    Arc::new(OutputImpl {
        verbosity: Mutex::new(Verbosity::Info),
    })
}
