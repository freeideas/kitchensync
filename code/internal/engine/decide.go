package engine

import (
	"kitchensync/internal/db"
	"kitchensync/internal/fsys"
	"kitchensync/internal/ts"
	"time"
)

func classifyEntry(peer *SyncPeer, relPath string, listing map[string]fsys.Entry, name string) *PeerState {
	snap := peer.Snapshot
	row, _ := snap.Lookup(relPath)

	entry, exists := listing[name]
	state := &PeerState{Peer: peer}

	if exists {
		state.IsLive = true
		state.IsDir = entry.IsDir
		state.ModTime = entry.ModTime
		if entry.IsDir {
			state.ByteSize = -1
		} else {
			state.ByteSize = entry.ByteSize
		}

		if row == nil {
			state.Classification = ClassNew
		} else if row.DeletedTime.Valid {
			state.Classification = ClassResurrection
		} else {
			// Compare mod_time
			snapTime, err := ts.Parse(row.ModTime)
			if err != nil {
				state.Classification = ClassModified
			} else {
				diff := entry.ModTime.Sub(snapTime)
				if diff < 0 {
					diff = -diff
				}
				if diff <= TimeTolerance {
					state.Classification = ClassUnchanged
				} else {
					state.Classification = ClassModified
				}
			}
		}
		if row != nil {
			state.LastSeen = db.FormatNullString(row.LastSeen)
			state.DeletedTime = db.FormatNullString(row.DeletedTime)
		}
	} else {
		state.IsLive = false
		if row == nil {
			state.Classification = ClassNoOpinion
		} else if row.DeletedTime.Valid {
			state.Classification = ClassDeleted
			state.DeletedTime = db.FormatNullString(row.DeletedTime)
			if row.DeletedTime.Valid {
				t, err := ts.Parse(row.DeletedTime.String)
				if err == nil {
					state.DeletionEst = t
				}
			}
			state.LastSeen = db.FormatNullString(row.LastSeen)
			state.ModTime, _ = ts.Parse(row.ModTime)
			state.ByteSize = row.ByteSize
			state.IsDir = row.ByteSize == -1
		} else {
			// Absent, no deleted_time -> absent-unconfirmed
			state.Classification = ClassAbsentUnconfirmed
			state.LastSeen = db.FormatNullString(row.LastSeen)
			state.ModTime, _ = ts.Parse(row.ModTime)
			state.ByteSize = row.ByteSize
			state.IsDir = row.ByteSize == -1
			if row.LastSeen.Valid {
				t, err := ts.Parse(row.LastSeen.String)
				if err == nil {
					state.DeletionEst = t
				}
			}
		}
	}

	return state
}

func decideFile(canonPeer *SyncPeer, contributing []*SyncPeer, states map[*SyncPeer]*PeerState) Decision {
	// Canon mode
	if canonPeer != nil {
		cs := states[canonPeer]
		if cs != nil && cs.IsLive {
			targets := peersNeedingUpdate(states, canonPeer, cs)
			return Decision{
				Action:  ActionPush,
				Type:    EntryFile,
				SrcPeer: canonPeer,
				SrcMod:  cs.ModTime,
				SrcSize: cs.ByteSize,
				Targets: targets,
			}
		}
		// Canon lacks file -> delete everywhere
		return Decision{Action: ActionDelete, Type: EntryFile}
	}

	// Gather voters (skip NoOpinion)
	voters := make(map[*SyncPeer]*PeerState)
	for _, p := range contributing {
		s := states[p]
		if s != nil && s.Classification != ClassNoOpinion {
			voters[p] = s
		}
	}

	if len(voters) == 0 {
		return Decision{Action: ActionDeleteSubordinatesOnly, Type: EntryFile}
	}

	live := make(map[*SyncPeer]*PeerState)
	deleted := make(map[*SyncPeer]*PeerState)
	absentUnconfirmed := make(map[*SyncPeer]*PeerState)

	for p, s := range voters {
		switch s.Classification {
		case ClassUnchanged, ClassModified, ClassNew, ClassResurrection:
			if s.IsLive {
				live[p] = s
			}
		case ClassDeleted:
			deleted[p] = s
		case ClassAbsentUnconfirmed:
			absentUnconfirmed[p] = s
		}
	}

	// Check all unchanged
	allUnchanged := true
	for _, s := range voters {
		if s.Classification != ClassUnchanged {
			allUnchanged = false
			break
		}
	}
	if allUnchanged {
		return Decision{Action: ActionNone, Type: EntryFile}
	}

	// Handle absent-unconfirmed (rule 4b)
	for p, s := range absentUnconfirmed {
		if s.LastSeen == nil {
			// Never confirmed present; re-enqueue
			live[p] = s
			continue
		}
		var maxLiveMtime time.Time
		for _, ls := range live {
			if ls.IsLive && ls.ModTime.After(maxLiveMtime) {
				maxLiveMtime = ls.ModTime
			}
		}
		lastSeenTime, _ := ts.Parse(*s.LastSeen)
		if !maxLiveMtime.IsZero() && lastSeenTime.After(maxLiveMtime.Add(TimeTolerance)) {
			// Confirmed deletion
			deleted[p] = s
			s.DeletionEst = lastSeenTime
		} else {
			live[p] = s
		}
	}

	// If live contains only non-live entries (absent-unconfirmed treated as live)
	if len(live) > 0 {
		allAbsent := true
		for _, s := range live {
			if s.IsLive {
				allAbsent = false
				break
			}
		}
		if allAbsent {
			return Decision{Action: ActionDelete, Type: EntryFile}
		}
	}

	if len(live) > 0 && len(deleted) == 0 {
		winner, wState := pickWinner(live)
		targets := peersNeedingUpdate(states, winner, wState)
		return Decision{
			Action:  ActionPush,
			Type:    EntryFile,
			SrcPeer: winner,
			SrcMod:  wState.ModTime,
			SrcSize: wState.ByteSize,
			Targets: targets,
		}
	}

	if len(deleted) > 0 && len(live) == 0 {
		return Decision{Action: ActionDelete, Type: EntryFile}
	}

	if len(live) > 0 && len(deleted) > 0 {
		var maxDeletionEst time.Time
		for _, s := range deleted {
			if s.DeletionEst.After(maxDeletionEst) {
				maxDeletionEst = s.DeletionEst
			}
		}
		var maxLiveMtime time.Time
		for _, s := range live {
			if s.IsLive && s.ModTime.After(maxLiveMtime) {
				maxLiveMtime = s.ModTime
			}
		}
		if maxDeletionEst.After(maxLiveMtime.Add(TimeTolerance)) {
			return Decision{Action: ActionDelete, Type: EntryFile}
		}
		// Existence wins
		winner, wState := pickWinner(live)
		targets := peersNeedingUpdate(states, winner, wState)
		return Decision{
			Action:  ActionPush,
			Type:    EntryFile,
			SrcPeer: winner,
			SrcMod:  wState.ModTime,
			SrcSize: wState.ByteSize,
			Targets: targets,
		}
	}

	return Decision{Action: ActionNone, Type: EntryFile}
}

func decideDir(canonPeer *SyncPeer, contributing []*SyncPeer, states map[*SyncPeer]*PeerState) Decision {
	if canonPeer != nil {
		cs := states[canonPeer]
		if cs != nil && cs.IsLive {
			return Decision{Action: ActionPush, Type: EntryDir}
		}
		return Decision{Action: ActionDelete, Type: EntryDir}
	}

	anyLive := false
	allDeleted := true
	hasVoter := false

	for _, p := range contributing {
		s := states[p]
		if s == nil || s.Classification == ClassNoOpinion {
			continue
		}
		hasVoter = true
		if s.IsLive {
			anyLive = true
			allDeleted = false
		} else if s.Classification != ClassDeleted {
			allDeleted = false
		}
	}

	if !hasVoter {
		return Decision{Action: ActionDeleteSubordinatesOnly, Type: EntryDir}
	}

	if anyLive {
		return Decision{Action: ActionPush, Type: EntryDir}
	}
	if allDeleted {
		return Decision{Action: ActionDelete, Type: EntryDir}
	}
	return Decision{Action: ActionNone, Type: EntryDir}
}

func pickWinner(live map[*SyncPeer]*PeerState) (*SyncPeer, *PeerState) {
	var maxMod time.Time
	for _, s := range live {
		if s.IsLive && s.ModTime.After(maxMod) {
			maxMod = s.ModTime
		}
	}

	// Find all within tolerance of max
	tied := make(map[*SyncPeer]*PeerState)
	for p, s := range live {
		if s.IsLive {
			diff := maxMod.Sub(s.ModTime)
			if diff < 0 {
				diff = -diff
			}
			if diff <= TimeTolerance {
				tied[p] = s
			}
		}
	}

	if len(tied) <= 1 {
		for p, s := range live {
			if s.IsLive && s.ModTime.Equal(maxMod) {
				return p, s
			}
		}
		// Fallback
		for p, s := range live {
			if s.IsLive {
				return p, s
			}
		}
	}

	// Tie-break by size (larger wins)
	var maxSize int64 = -1
	var winner *SyncPeer
	var winnerState *PeerState
	for p, s := range tied {
		if s.ByteSize > maxSize {
			maxSize = s.ByteSize
			winner = p
			winnerState = s
		}
	}
	return winner, winnerState
}

func peersNeedingUpdate(allStates map[*SyncPeer]*PeerState, winner *SyncPeer, wState *PeerState) []*SyncPeer {
	var targets []*SyncPeer
	for p, s := range allStates {
		if p == winner {
			continue
		}
		if p.IsSub() {
			continue // handled separately
		}
		if s.IsLive && s.ByteSize == wState.ByteSize {
			diff := wState.ModTime.Sub(s.ModTime)
			if diff < 0 {
				diff = -diff
			}
			if diff <= TimeTolerance {
				continue // already matches
			}
		}
		targets = append(targets, p)
	}
	return targets
}

