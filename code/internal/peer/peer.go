package peer

import (
	"kitchensync/internal/cli"
	"kitchensync/internal/fsutil"
	"kitchensync/internal/pool"
	"kitchensync/internal/snapshot"
	"kitchensync/internal/urlutil"
)

// Peer represents a sync peer with its connections and snapshot.
type Peer struct {
	Arg       cli.PeerArg
	ActiveURL *urlutil.NormalizedURL
	Reachable bool
	Pool      *pool.ConnPool
	Snap      *snapshot.DB
	ListConn  fsutil.PeerFS // dedicated listing connection

	// Auto-subordinate: no snapshot found on peer
	AutoSubordinate bool
}

// IsSubordinate returns true if the peer is subordinate (explicit or auto).
// Canon peers are never subordinate, even if auto-subordinate is set.
func (p *Peer) IsSubordinate() bool {
	if p.Arg.Role == cli.RoleCanon {
		return false
	}
	return p.Arg.Role == cli.RoleSubordinate || p.AutoSubordinate
}

// IsCanon returns true if the peer is the canon peer.
func (p *Peer) IsCanon() bool {
	return p.Arg.Role == cli.RoleCanon
}

// Label returns a display name for the peer.
func (p *Peer) Label() string {
	if p.ActiveURL != nil {
		return p.ActiveURL.String()
	}
	if len(p.Arg.URLs) > 0 {
		return p.Arg.URLs[0].Raw
	}
	return "(unknown)"
}
