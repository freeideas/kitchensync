package sync

import (
	"path"

	"kitchensync/internal/log"
	"kitchensync/internal/peer"
	"kitchensync/internal/snapshot"
	"kitchensync/internal/timestamp"
)

// Displace moves a file or directory to BAK/ on the given peer.
// Uses the peer's listing connection (inline during walk).
func Displace(p *peer.Peer, relPath string) error {
	ts := timestamp.FormatTime(timestamp.Now())
	basename := path.Base(relPath)
	parentDir := path.Dir(relPath)
	if parentDir == "." {
		parentDir = ""
	}
	bakPath := path.Join(parentDir, ".kitchensync/BAK", ts, basename)

	err := p.ListConn.Rename(relPath, bakPath)
	if err != nil {
		log.Error("displace %s on %s: %v", relPath, p.Label(), err)
	}
	return err
}

// MarkDeleted sets the snapshot entry as deleted for a peer.
func MarkDeleted(p *peer.Peer, relPath string) {
	row, err := p.Snap.Get(relPath)
	if err != nil || row == nil {
		return
	}
	if row.DeletedTime.Valid {
		return // already tombstoned
	}
	if row.LastSeen.Valid {
		p.Snap.SetDeletedTimeValue(relPath, row.LastSeen.String)
	} else {
		nowStr := snapshot.NowStr()
		p.Snap.SetDeletedTimeValue(relPath, nowStr)
	}
}

// CascadeTombstones marks all descendants of a displaced directory as deleted.
func CascadeTombstones(p *peer.Peer, relPath string) {
	nowStr := snapshot.NowStr()
	p.Snap.CascadeTombstones(relPath, nowStr)
}
