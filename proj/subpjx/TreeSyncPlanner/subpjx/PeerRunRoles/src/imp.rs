use std::sync::Arc;
use crate::api::*;

struct PeerRunRolesImpl;

impl PeerRunRoles for PeerRunRolesImpl {
    fn classify_startup_roles(&self, request: PeerRunRolesRequest) -> PeerRunRolesResult {
        for peer in &request.peers {
            if peer.role_marker == StartupPeerRoleMarker::Canon
                && peer.reachability == StartupPeerReachability::Unreachable
            {
                return PeerRunRolesResult::FatalStartup(
                    PeerRunRolesFatalStartup::UnreachableCanon {
                        exit_status: 1,
                        canon_peer_identity: peer.peer_identity.clone(),
                    },
                );
            }
        }

        let has_canon = request
            .peers
            .iter()
            .any(|peer| peer.role_marker == StartupPeerRoleMarker::Canon);
        let has_reachable_snapshot = request.peers.iter().any(|peer| {
            peer.reachability == StartupPeerReachability::Reachable
                && peer.had_snapshot_database_at_startup
        });

        if !has_reachable_snapshot && !has_canon {
            return PeerRunRolesResult::FatalStartup(
                PeerRunRolesFatalStartup::FirstSyncRequiresCanon {
                    exit_status: 1,
                    stdout_line: "First sync? Mark the authoritative peer with a leading +"
                        .to_string(),
                },
            );
        }

        let active_peers: Vec<PeerRunRoleFact> = request
            .peers
            .into_iter()
            .filter(|peer| peer.reachability == StartupPeerReachability::Reachable)
            .map(|peer| {
                let is_canon = peer.role_marker == StartupPeerRoleMarker::Canon;
                let role = if is_canon {
                    PeerRunRole::Contributing
                } else if peer.role_marker == StartupPeerRoleMarker::Subordinate
                    || !peer.had_snapshot_database_at_startup
                {
                    PeerRunRole::Subordinate
                } else {
                    PeerRunRole::Contributing
                };

                PeerRunRoleFact {
                    peer_identity: peer.peer_identity,
                    is_canon,
                    role,
                    is_active_target: true,
                }
            })
            .collect();

        if !active_peers
            .iter()
            .any(|peer| peer.role == PeerRunRole::Contributing)
        {
            return PeerRunRolesResult::FatalStartup(
                PeerRunRolesFatalStartup::NoContributingPeer {
                    exit_status: 1,
                    stdout_line:
                        "No contributing peer reachable - cannot make sync decisions".to_string(),
                },
            );
        }

        PeerRunRolesResult::Success(PeerRunRolesFacts { active_peers })
    }
}

pub fn new() -> std::sync::Arc<dyn PeerRunRoles> {
    Arc::new(PeerRunRolesImpl)
}
