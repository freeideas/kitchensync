use std::sync::Arc;
use crate::api::*;

struct DisplacementImpl;

impl Displacement for DisplacementImpl {
    fn displace(
        &self,
        transport: &dyn transport::Transport,
        output: &dyn output::Output,
        peer: &transport::PeerHandle,
        parent: &str,
        basename: &str,
        timestamp: &str,
        dry_run: bool,
    ) -> DisplaceOutcome {
        if dry_run {
            return DisplaceOutcome::Displaced;
        }

        // create_dir creates missing parent directories per transport spec (022.9),
        // so one call covers .kitchensync/, BAK/, and the timestamp directory.
        let bak_ts_dir = format!("{}/.kitchensync/BAK/{}", parent, timestamp);
        let _ = transport.create_dir(peer, &bak_ts_dir);

        let src = format!("{}/{}", parent, basename);
        let dst = format!("{}/.kitchensync/BAK/{}/{}", parent, timestamp, basename);

        match transport.rename(peer, &src, &dst) {
            Ok(()) => DisplaceOutcome::Displaced,
            Err(e) => {
                let reason = match e {
                    transport::TransportError::NotFound => "not found",
                    transport::TransportError::PermissionDenied => "permission denied",
                    transport::TransportError::Io => "I/O error",
                };
                output.diagnostic(&format!(
                    "displacement failed: could not rename {} to {}: {}",
                    src, dst, reason
                ));
                DisplaceOutcome::LeftInPlace
            }
        }
    }
}

pub fn new() -> Arc<dyn Displacement> {
    Arc::new(DisplacementImpl)
}
