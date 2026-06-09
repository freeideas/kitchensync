use std::sync::Arc;
use crate::api::*;

const FIVE_SEC: i64 = 5_000_000; // 5 seconds in microseconds

// ---- timestamp helpers ----

fn parse_ts(s: &str) -> Option<i64> {
    // YYYY-MM-DD_HH-mm-ss_ffffffZ  (27 chars)
    if s.len() < 27 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: i64 = s.get(14..16)?.parse().ok()?;
    let sc: i64 = s.get(17..19)?.parse().ok()?;
    let us: i64 = s.get(20..26)?.parse().ok()?;
    Some(days_since_ref(y, mo, d) * 86_400_000_000
        + h * 3_600_000_000
        + mi * 60_000_000
        + sc * 1_000_000
        + us)
}

const MDAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_since_ref(y: i64, mo: u32, d: u32) -> i64 {
    let y1 = y - 1;
    let base = 365 * y1 + y1 / 4 - y1 / 100 + y1 / 400;
    let mut dm = (d - 1) as i64;
    for m in 1..mo {
        dm += MDAYS[(m - 1) as usize] as i64;
    }
    if mo > 2 && is_leap(y) {
        dm += 1;
    }
    base + dm
}

fn within_5s(a: &str, b: &str) -> bool {
    match (parse_ts(a), parse_ts(b)) {
        (Some(ta), Some(tb)) => (ta - tb).abs() <= FIVE_SEC,
        _ => false,
    }
}

// ---- file winner selection ----

// versions: (peer_url, byte_size, mod_time)
// Returns the winner: newest mod_time, ties broken by largest byte_size.
fn pick_winner<'a>(vers: &[(&'a str, i64, &'a str)]) -> Option<(&'a str, i64, &'a str)> {
    let max_ts = vers.iter().filter_map(|&(_, _, mt)| parse_ts(mt)).max()?;
    let mut best: Option<(&str, i64, &str)> = None;
    for &(peer, sz, mt) in vers {
        if parse_ts(mt).map_or(false, |t| max_ts - t <= FIVE_SEC) {
            if best.map_or(true, |(_, bs, _)| sz > bs) {
                best = Some((peer, sz, mt));
            }
        }
    }
    best
}

fn matches_winner(live: &LiveEntry, w_sz: i64, w_mt: &str) -> bool {
    match live {
        LiveEntry::File { byte_size, mod_time } => {
            *byte_size == w_sz && within_5s(mod_time, w_mt)
        }
        _ => false,
    }
}

fn mk(p: &PeerInput, displace: bool, conform: Conform) -> PeerOutcome {
    PeerOutcome { peer: p.peer.clone(), displace, conform }
}

// ---- canon decision ----

fn decide_canon(peers: &[PeerInput], ci: usize) -> Decision {
    let c = &peers[ci];
    match &c.live {
        LiveEntry::File { byte_size: w_sz, mod_time: w_mt } => {
            let actions = peers.iter().enumerate().map(|(i, p)| {
                if i == ci {
                    return mk(p, false, Conform::Nothing);
                }
                let displace = matches!(p.live, LiveEntry::Directory); // 012.8
                let conf = if matches_winner(&p.live, *w_sz, w_mt) {
                    Conform::Nothing // 011.15
                } else {
                    Conform::CopyWinner // 011.1
                };
                mk(p, displace, conf)
            }).collect();
            Decision { agreed_type: DecidedType::File, winner: Some(c.peer.clone()), actions }
        }
        LiveEntry::Directory => {
            let actions = peers.iter().enumerate().map(|(i, p)| {
                if i == ci {
                    return mk(p, false, Conform::Nothing);
                }
                let displace = matches!(p.live, LiveEntry::File { .. }); // 012.10
                let conf = if matches!(p.live, LiveEntry::Directory) {
                    Conform::Nothing
                } else {
                    Conform::CreateDirectory // 012.11
                };
                mk(p, displace, conf)
            }).collect();
            Decision { agreed_type: DecidedType::Directory, winner: None, actions }
        }
        LiveEntry::Absent => {
            // 012.12: canon lacks path; displace everything on every other peer.
            let actions = peers.iter().enumerate().map(|(i, p)| {
                if i == ci {
                    return mk(p, false, Conform::Nothing);
                }
                mk(p, !matches!(p.live, LiveEntry::Absent), Conform::Nothing)
            }).collect();
            Decision { agreed_type: DecidedType::Absent, winner: None, actions }
        }
    }
}

// ---- file decision (no canon) ----

fn decide_file(peers: &[PeerInput], contrib: &[usize]) -> Decision {
    // Collect file versions from contributing peers only (007.2).
    let vers: Vec<(&str, i64, &str)> = contrib.iter().filter_map(|&i| {
        if let LiveEntry::File { byte_size, mod_time } = &peers[i].live {
            Some((peers[i].peer.as_str(), *byte_size, mod_time.as_str()))
        } else {
            None
        }
    }).collect();

    let max_file_ts: Option<i64> = vers.iter()
        .filter_map(|&(_, _, mt)| parse_ts(mt))
        .max();

    // Deletion votes from contributing absent peers.
    let mut del_votes: Vec<i64> = Vec::new();
    for &i in contrib {
        if !matches!(peers[i].live, LiveEntry::Absent) {
            continue;
        }
        match &peers[i].row {
            None => {} // no-opinion: no vote (010.8, 011.13)
            Some(row) => {
                if let Some(dt) = &row.deleted_time {
                    // Deleted peer: cast vote (010.6, 011.7)
                    if let Some(t) = parse_ts(dt) {
                        del_votes.push(t);
                    }
                } else {
                    // Absent-unconfirmed: vote only when last_seen > max_file by >5s (011.10)
                    if let (Some(ls), Some(mft)) = (row.last_seen.as_deref(), max_file_ts) {
                        if let Some(lst) = parse_ts(ls) {
                            if lst - mft > FIVE_SEC {
                                del_votes.push(lst);
                            }
                        }
                    }
                }
            }
        }
    }

    let max_del: Option<i64> = del_votes.iter().copied().max();
    let winner = pick_winner(&vers);

    // Deletion wins when its estimate exceeds the winner's mod_time by >5s (011.8).
    let del_wins = match (winner, max_del) {
        (Some((_, _, wmt)), Some(dt)) => {
            parse_ts(wmt).map_or(false, |ft| dt - ft > FIVE_SEC)
        }
        (None, Some(_)) => true, // no live file but deletion vote exists
        _ => false,
    };

    let (w_sz, w_mt, winner_peer): (i64, &str, Option<String>) = if !del_wins {
        match winner {
            Some((wp, wz, wm)) => (wz, wm, Some(wp.to_owned())),
            None => (0, "", None),
        }
    } else {
        (0, "", None)
    };
    let is_file = winner_peer.is_some();

    let actions: Vec<PeerOutcome> = peers.iter().map(|p| {
        if is_file {
            let displace = matches!(p.live, LiveEntry::Directory);
            let conf = if matches_winner(&p.live, w_sz, w_mt) {
                Conform::Nothing // 011.15: already matches winner
            } else {
                Conform::CopyWinner // 011.4, 011.14: all peers receive winner
            };
            mk(p, displace, conf)
        } else {
            // Deletion wins or no file: displace any live entry.
            mk(p, !matches!(p.live, LiveEntry::Absent), Conform::Nothing)
        }
    }).collect();

    let agreed_type = if is_file { DecidedType::File } else { DecidedType::Absent };
    Decision { agreed_type, winner: winner_peer, actions }
}

// ---- directory decision (no canon) ----

fn decide_directory(peers: &[PeerInput], contrib: &[usize], c_dirs: &[usize]) -> Decision {
    if !c_dirs.is_empty() {
        // 012.1: any contributing peer has live directory -> create on every peer that lacks it.
        let actions = peers.iter().map(|p| {
            let displace = matches!(p.live, LiveEntry::File { .. }); // wrong type (012.16)
            let conf = if matches!(p.live, LiveEntry::Directory) {
                Conform::Nothing
            } else {
                Conform::CreateDirectory // 012.1, 012.17
            };
            mk(p, displace, conf)
        }).collect();
        return Decision { agreed_type: DecidedType::Directory, winner: None, actions };
    }

    // No contributing peer has a live directory.
    let has_dir_row = contrib.iter().any(|&i| {
        peers[i].row.as_ref().map_or(false, |r| r.byte_size == -1)
    });

    if has_dir_row {
        // 012.3: at least one contributing peer had a directory row and is now absent;
        // displace from every peer that still has the directory.
        // 012.4: contributing peers with no row neither vote nor block.
        let actions = peers.iter().map(|p| {
            mk(p, matches!(p.live, LiveEntry::Directory), Conform::Nothing)
        }).collect();
        Decision { agreed_type: DecidedType::Absent, winner: None, actions }
    } else {
        // 012.5: no contributing peer has it live and none has a row;
        // displace only from subordinate peers.
        let actions = peers.iter().map(|p| {
            let displace = matches!(p.role, PeerRole::Subordinate)
                && matches!(p.live, LiveEntry::Directory);
            mk(p, displace, Conform::Nothing)
        }).collect();
        Decision { agreed_type: DecidedType::Absent, winner: None, actions }
    }
}

// ---- type conflict: file and directory on contributing peers (no canon) ----

fn decide_conflict(peers: &[PeerInput], c_files: &[usize], c_dirs: &[usize]) -> Decision {
    // 012.13: file wins; select winner from contributing file entries.
    let vers: Vec<(&str, i64, &str)> = c_files.iter().filter_map(|&i| {
        if let LiveEntry::File { byte_size, mod_time } = &peers[i].live {
            Some((peers[i].peer.as_str(), *byte_size, mod_time.as_str()))
        } else {
            None
        }
    }).collect();

    let (w_sz, w_mt, winner_peer) = match pick_winner(&vers) {
        Some((wp, wz, wm)) => (wz, wm, Some(wp.to_owned())),
        None => (0, "", None),
    };

    let actions: Vec<PeerOutcome> = peers.iter().enumerate().map(|(i, p)| {
        if c_dirs.contains(&i) {
            // 012.13: displace contributing directory, then copy winner (012.14).
            mk(p, true, Conform::CopyWinner)
        } else if matches_winner(&p.live, w_sz, w_mt) {
            mk(p, false, Conform::Nothing) // 011.15
        } else {
            // Subordinate wrong-type directories (012.16) or any peer needing the winner.
            let displace = matches!(p.live, LiveEntry::Directory);
            mk(p, displace, Conform::CopyWinner) // 012.14, 012.17
        }
    }).collect();

    Decision { agreed_type: DecidedType::File, winner: winner_peer, actions }
}

// ---- top-level dispatch ----

fn decide_path(peers: &[PeerInput]) -> Decision {
    // Canon present: its state wins unconditionally (007.1).
    if let Some(ci) = peers.iter().position(|p| matches!(p.role, PeerRole::Canon)) {
        return decide_canon(peers, ci);
    }

    let contrib: Vec<usize> = peers.iter().enumerate()
        .filter(|(_, p)| matches!(p.role, PeerRole::Contributing))
        .map(|(i, _)| i)
        .collect();

    let c_files: Vec<usize> = contrib.iter().copied()
        .filter(|&i| matches!(peers[i].live, LiveEntry::File { .. }))
        .collect();
    let c_dirs: Vec<usize> = contrib.iter().copied()
        .filter(|&i| matches!(peers[i].live, LiveEntry::Directory))
        .collect();

    // Type conflict: contributing peers hold both files and directories.
    // Subordinate peer files never trigger this (012.15).
    if !c_files.is_empty() && !c_dirs.is_empty() {
        return decide_conflict(peers, &c_files, &c_dirs);
    }

    // Directory path: contributing dir present, or no contributing files but dir row exists.
    let has_c_dir_row = contrib.iter().any(|&i| {
        peers[i].row.as_ref().map_or(false, |r| r.byte_size == -1)
    });
    if !c_dirs.is_empty() || (c_files.is_empty() && has_c_dir_row) {
        return decide_directory(peers, &contrib, &c_dirs);
    }

    // File path (including all-absent with file context or no context).
    decide_file(peers, &contrib)
}

// ---- struct and factory ----

struct DecisionRulesImpl;

impl DecisionRules for DecisionRulesImpl {
    fn decide(&self, peers: &[PeerInput]) -> Decision {
        decide_path(peers)
    }
}

pub fn new() -> Arc<dyn DecisionRules> {
    Arc::new(DecisionRulesImpl)
}
