package engine

import (
	"kitchensync/internal/args"
	"kitchensync/internal/db"
	"kitchensync/internal/fsys"
	"kitchensync/internal/pool"
	"time"
)

type ActionType int

const (
	ActionNone ActionType = iota
	ActionPush
	ActionDelete
	ActionDeleteSubordinatesOnly
)

type EntryType int

const (
	EntryFile EntryType = iota
	EntryDir
)

type Classification int

const (
	ClassUnchanged Classification = iota
	ClassModified
	ClassResurrection
	ClassNew
	ClassDeleted
	ClassAbsentUnconfirmed
	ClassNoOpinion
)

const TimeTolerance = 5 * time.Second

type PeerState struct {
	Peer           *SyncPeer
	Classification Classification
	IsLive         bool
	IsDir          bool
	ModTime        time.Time
	ByteSize       int64
	LastSeen       *string
	DeletedTime    *string
	DeletionEst    time.Time
}

type Decision struct {
	Action    ActionType
	Type      EntryType
	SrcPeer   *SyncPeer
	SrcMod    time.Time
	SrcSize   int64
	Targets   []*SyncPeer
}

type SyncPeer struct {
	Config        *args.Peer
	ActiveURL     string
	Reachable     bool
	ListingFS     fsys.PeerFS
	Pool          *pool.ConnPool
	Snapshot      *db.SnapshotDB
	IsSubordinate bool
	AutoSub       bool
	Password      string
}

func (p *SyncPeer) IsCanon() bool {
	return p.Config.IsCanon
}

func (p *SyncPeer) IsSub() bool {
	return p.IsSubordinate || p.AutoSub
}
