package sync

import (
	"time"

	"kitchensync/internal/peer"
)

// Decide makes a decision for a file entry based on contributing peer states.
func Decide(states map[*peer.Peer]PeerState, canonPeer *peer.Peer) Decision {
	// Canon mode
	if canonPeer != nil {
		return decideCanon(states, canonPeer)
	}
	return decideNormal(states)
}

func decideCanon(states map[*peer.Peer]PeerState, canon *peer.Peer) Decision {
	cs, hasCanon := states[canon]
	if !hasCanon || !cs.IsLive {
		// Canon lacks file -> delete everywhere
		return Decision{Type: EntryFile, Act: ActionDelete}
	}
	// Canon has file -> push to all others that differ
	var targets []*peer.Peer
	for p, s := range states {
		if p == canon {
			continue
		}
		if !s.IsLive || !withinTolerance(s.ModTime, cs.ModTime) || s.ByteSize != cs.ByteSize {
			targets = append(targets, p)
		}
	}
	return Decision{Type: EntryFile, Act: ActionPush, Src: canon, Targets: targets, ModTime: cs.ModTime, Size: cs.ByteSize}
}

func decideNormal(states map[*peer.Peer]PeerState) Decision {
	// Filter out NO_OPINION
	voters := make(map[*peer.Peer]PeerState)
	for p, s := range states {
		if s.Class != ClassNoOpinion {
			voters[p] = s
		}
	}

	if len(voters) == 0 {
		return Decision{Type: EntryFile, Act: ActionDeleteSubordinatesOnly}
	}

	live := make(map[*peer.Peer]PeerState)
	deleted := make(map[*peer.Peer]PeerState)
	absentUnconfirmed := make(map[*peer.Peer]PeerState)

	for p, s := range voters {
		switch {
		case s.IsLive:
			live[p] = s
		case s.Class == ClassDeleted:
			deleted[p] = s
		case s.Class == ClassAbsentUnconfirmed:
			absentUnconfirmed[p] = s
		}
	}

	// Rule 1: all unchanged -> no action
	allUnchanged := true
	for _, s := range voters {
		if s.Class != ClassUnchanged {
			allUnchanged = false
			break
		}
	}
	if allUnchanged {
		// Still compute modtime/size for snapshot updates
		for _, s := range voters {
			return Decision{Type: EntryFile, Act: ActionNone, ModTime: s.ModTime, Size: s.ByteSize}
		}
	}

	// Handle absent-unconfirmed (rule 4b)
	maxLiveMtime := maxModTime(live)
	for p, s := range absentUnconfirmed {
		if s.LastSeen == nil {
			// Never confirmed present — pending copy that never completed. Re-enqueue.
			live[p] = s
			continue
		}
		if !maxLiveMtime.IsZero() && s.LastSeen.After(maxLiveMtime.Add(Tolerance)) {
			// Confirmed deletion
			est := *s.LastSeen
			s.DeletionEstimate = &est
			s.Class = ClassDeleted
			deleted[p] = s
		} else {
			// Failed copy or never received
			live[p] = s
		}
	}

	// If live contains only entries that are not physically live, treat as DELETE
	if len(live) > 0 {
		allAbsent := true
		for _, s := range live {
			if s.IsLive {
				allAbsent = false
				break
			}
		}
		if allAbsent {
			return Decision{Type: EntryFile, Act: ActionDelete}
		}
	}

	if len(live) > 0 && len(deleted) == 0 {
		return pickWinner(live, states)
	}

	if len(deleted) > 0 && len(live) == 0 {
		return Decision{Type: EntryFile, Act: ActionDelete}
	}

	if len(live) > 0 && len(deleted) > 0 {
		// Rule 4: compare deletion estimate vs live mod_time
		maxDelEst := maxDeletionEstimate(deleted)
		maxLive := maxModTime(live)
		if maxDelEst.After(maxLive.Add(Tolerance)) {
			return Decision{Type: EntryFile, Act: ActionDelete}
		}
		// Rule 6: ties favor existence
		return pickWinner(live, states)
	}

	return Decision{Type: EntryFile, Act: ActionNone}
}

func pickWinner(live map[*peer.Peer]PeerState, allStates map[*peer.Peer]PeerState) Decision {
	// Pick by mod_time (newest wins)
	maxMtime := maxModTime(live)

	// Tolerance: anyone within 5s of max
	tied := make(map[*peer.Peer]PeerState)
	for p, s := range live {
		if !s.IsLive {
			continue
		}
		if maxMtime.Sub(s.ModTime) <= Tolerance {
			tied[p] = s
		}
	}

	if len(tied) > 1 {
		// Rule 5: same mod_time, larger file wins
		maxSize := int64(-1)
		for _, s := range tied {
			if s.ByteSize > maxSize {
				maxSize = s.ByteSize
			}
		}
		sizeTied := make(map[*peer.Peer]PeerState)
		for p, s := range tied {
			if s.ByteSize == maxSize {
				sizeTied[p] = s
			}
		}

		// Check if all live peers agree (same mod_time and byte_size within tolerance)
		allLiveAgree := true
		for p, s := range live {
			if !s.IsLive {
				continue
			}
			if _, ok := sizeTied[p]; !ok {
				allLiveAgree = false
				break
			}
		}
		if allLiveAgree && len(sizeTied) == countLive(live) {
			return Decision{Type: EntryFile, Act: ActionNone, ModTime: maxMtime, Size: maxSize}
		}

		// Winner is largest among tied
		var winnerPeer *peer.Peer
		for p, s := range tied {
			if s.ByteSize == maxSize {
				winnerPeer = p
				break
			}
		}
		return buildPushDecision(winnerPeer, sizeTied[winnerPeer], allStates)
	}

	// Single winner
	var winnerPeer *peer.Peer
	var winnerState PeerState
	for p, s := range live {
		if s.IsLive && (winnerPeer == nil || s.ModTime.After(winnerState.ModTime)) {
			winnerPeer = p
			winnerState = s
		}
	}
	return buildPushDecision(winnerPeer, winnerState, allStates)
}

func buildPushDecision(winner *peer.Peer, ws PeerState, allStates map[*peer.Peer]PeerState) Decision {
	var targets []*peer.Peer
	for p, s := range allStates {
		if p == winner {
			continue
		}
		// Skip peers that already have matching content
		if s.IsLive && withinTolerance(s.ModTime, ws.ModTime) && s.ByteSize == ws.ByteSize {
			continue
		}
		targets = append(targets, p)
	}

	act := ActionPush
	if len(targets) == 0 {
		act = ActionNone
	}
	return Decision{Type: EntryFile, Act: act, Src: winner, Targets: targets, ModTime: ws.ModTime, Size: ws.ByteSize}
}

func countLive(m map[*peer.Peer]PeerState) int {
	n := 0
	for _, s := range m {
		if s.IsLive {
			n++
		}
	}
	return n
}

func maxModTime(m map[*peer.Peer]PeerState) time.Time {
	var max time.Time
	for _, s := range m {
		if s.IsLive && s.ModTime.After(max) {
			max = s.ModTime
		}
	}
	return max
}

func maxDeletionEstimate(m map[*peer.Peer]PeerState) time.Time {
	var max time.Time
	for _, s := range m {
		if s.DeletionEstimate != nil && s.DeletionEstimate.After(max) {
			max = *s.DeletionEstimate
		}
	}
	return max
}

func withinTolerance(a, b time.Time) bool {
	diff := a.Sub(b)
	if diff < 0 {
		diff = -diff
	}
	return diff <= Tolerance
}

// DecideDir makes a decision for a directory entry.
func DecideDir(states map[*peer.Peer]PeerState, canonPeer *peer.Peer) Decision {
	if canonPeer != nil {
		cs, hasCanon := states[canonPeer]
		if !hasCanon || !cs.IsLive {
			return Decision{Type: EntryDir, Act: ActionDelete}
		}
		return Decision{Type: EntryDir, Act: ActionPush}
	}

	// Any contributing peer has it -> create on peers that lack it
	anyLive := false
	allDeleted := true
	for _, s := range states {
		if s.Class == ClassNoOpinion {
			continue
		}
		if s.IsLive {
			anyLive = true
			allDeleted = false
		} else if s.Class != ClassDeleted {
			allDeleted = false
		}
	}

	if anyLive {
		return Decision{Type: EntryDir, Act: ActionPush}
	}
	if allDeleted {
		return Decision{Type: EntryDir, Act: ActionDelete}
	}
	return Decision{Type: EntryDir, Act: ActionNone}
}
