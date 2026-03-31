package sync

import (
	"time"

	"kitchensync/internal/fsutil"
	"kitchensync/internal/peer"
	"kitchensync/internal/snapshot"
)

// ClassifyEntry classifies a file entry on a contributing peer by comparing
// the filesystem state to the peer's snapshot row.
func ClassifyEntry(p *peer.Peer, relPath string, listing map[string]fsutil.DirEntry) PeerState {
	entry, exists := listing[snapshot.BaseName(relPath)]

	row, err := p.Snap.Get(relPath)
	if err != nil {
		row = nil
	}

	if exists && !entry.IsDir {
		// Live file
		if row == nil {
			// No snapshot row -> New
			return PeerState{
				Class:    ClassNew,
				ModTime:  entry.ModTime,
				ByteSize: entry.ByteSize,
				IsLive:   true,
			}
		}

		if row.DeletedTime.Valid {
			// Was tombstoned -> Resurrection
			return PeerState{
				Class:    ClassResurrection,
				ModTime:  entry.ModTime,
				ByteSize: entry.ByteSize,
				IsLive:   true,
			}
		}

		// Compare mod_time
		snapTime, _ := snapshot.ParseModTime(row.ModTime)
		if withinTolerance(entry.ModTime, snapTime) {
			return PeerState{
				Class:    ClassUnchanged,
				ModTime:  entry.ModTime,
				ByteSize: entry.ByteSize,
				IsLive:   true,
			}
		}

		return PeerState{
			Class:    ClassModified,
			ModTime:  entry.ModTime,
			ByteSize: entry.ByteSize,
			IsLive:   true,
		}
	}

	// Absent
	if row == nil {
		return PeerState{Class: ClassNoOpinion}
	}

	if row.DeletedTime.Valid {
		delTime, _ := snapshot.ParseModTime(row.DeletedTime.String)
		return PeerState{
			Class:            ClassDeleted,
			DeletionEstimate: &delTime,
		}
	}

	// Absent, snapshot row with no deleted_time -> Absent-unconfirmed
	var lastSeen *time.Time
	if row.LastSeen.Valid {
		t, _ := snapshot.ParseModTime(row.LastSeen.String)
		lastSeen = &t
	}
	return PeerState{
		Class:    ClassAbsentUnconfirmed,
		LastSeen: lastSeen,
	}
}

// ClassifyDirEntry classifies a directory entry on a contributing peer.
func ClassifyDirEntry(p *peer.Peer, relPath string, listing map[string]fsutil.DirEntry) PeerState {
	entry, exists := listing[snapshot.BaseName(relPath)]

	row, err := p.Snap.Get(relPath)
	if err != nil {
		row = nil
	}

	if exists && entry.IsDir {
		return PeerState{
			Class:  ClassNew, // or unchanged, doesn't matter for dirs
			IsLive: true,
		}
	}

	if row == nil {
		return PeerState{Class: ClassNoOpinion}
	}

	if row.DeletedTime.Valid {
		delTime, _ := snapshot.ParseModTime(row.DeletedTime.String)
		return PeerState{
			Class:            ClassDeleted,
			DeletionEstimate: &delTime,
		}
	}

	return PeerState{Class: ClassAbsentUnconfirmed}
}
