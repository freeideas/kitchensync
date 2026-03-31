package sync

import (
	"time"

	"kitchensync/internal/peer"
)

// Classification of an entry on a peer.
type Classification int

const (
	ClassUnchanged        Classification = iota
	ClassModified                        // live, different mod_time
	ClassResurrection                    // live, was tombstoned
	ClassNew                             // live, no snapshot row
	ClassDeleted                         // absent, tombstoned
	ClassAbsentUnconfirmed               // absent, snapshot row with no deleted_time
	ClassNoOpinion                       // absent, no snapshot row
)

// PeerState is the classification of an entry on a contributing peer.
type PeerState struct {
	Class            Classification
	ModTime          time.Time
	ByteSize         int64
	IsLive           bool
	LastSeen         *time.Time
	DeletionEstimate *time.Time
}

// Action is what to do with an entry.
type Action int

const (
	ActionNone                  Action = iota
	ActionPush                         // push from src to targets
	ActionDelete                       // delete everywhere
	ActionDeleteSubordinatesOnly       // no contributing peer knows it
)

// EntryType is file or directory.
type EntryType int

const (
	EntryFile EntryType = iota
	EntryDir
)

// Decision is the outcome for a single entry.
type Decision struct {
	Type    EntryType
	Act     Action
	Src     *peer.Peer   // source peer for PUSH
	Targets []*peer.Peer // peers that need the file
	ModTime time.Time    // winning mod_time
	Size    int64        // winning byte_size
}

const Tolerance = 5 * time.Second
