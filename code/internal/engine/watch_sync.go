package engine

import (
	"io"
	"kitchensync/internal/db"
	"kitchensync/internal/fsys"
	"kitchensync/internal/ignore"
	"kitchensync/internal/logx"
	"kitchensync/internal/ts"
	"kitchensync/internal/urlnorm"
	"path"
	"strings"
	"sync"
	"time"
)

// WatchCopyQueue is a long-lived copy queue for the watch session.
// It is separate from the initial-sync CopyQueue (which is closed after Run()).
type WatchSession struct {
	eng       *Engine
	copyQueue *CopyQueue
}

func (e *Engine) NewWatchSession() *WatchSession {
	ws := &WatchSession{eng: e}
	if !e.DryRun {
		ws.copyQueue = NewCopyQueue(e.Opts.MC*2, e.checkCopyCheckpoint)
	}
	return ws
}

func (ws *WatchSession) Close() {
	if ws.copyQueue != nil {
		ws.copyQueue.CloseAndWait(30 * time.Second)
	}
}

// SyncEntry handles a single watch event for relPath.
// It gathers state from all peers, makes a decision, and executes it.
func (ws *WatchSession) SyncEntry(relPath string) {
	e := ws.eng

	reachable := e.reachablePeers()
	contributing := e.contributingPeers(reachable)
	subordinates := e.subordinatePeers(reachable)
	canon := e.canonPeer(reachable)

	if len(contributing) == 0 {
		return
	}

	// Check ignore rules
	name := path.Base(relPath)
	dirPath := path.Dir(relPath)
	if dirPath == "." {
		dirPath = ""
	}

	// Load .syncignore from the directory if available
	rules := ignore.NewRules()
	for _, p := range contributing {
		syncIgnorePath := db.RelPath(dirPath, ".syncignore")
		reader, err := p.ListingFS.ReadFile(syncIgnorePath)
		if err != nil {
			continue
		}
		data, err := io.ReadAll(reader)
		reader.Close()
		if err != nil {
			continue
		}
		rules = rules.Merge(string(data))
		break
	}

	// Check if file matches ignore rules
	// We need to check if the entry is a dir for matching purposes
	isDir := false
	for _, p := range contributing {
		entry, _ := p.ListingFS.Stat(relPath)
		if entry != nil {
			isDir = entry.IsDir
			break
		}
	}
	if rules.Matches(name, isDir) {
		return
	}

	parentRelPath := dirPath
	if parentRelPath == "" {
		parentRelPath = db.SentinelPath
	}

	// Gather states from all peers using stat() for local, snapshot for remote
	states := make(map[*SyncPeer]*PeerState)
	var entryIsDir *bool

	for _, p := range contributing {
		listing := ws.gatherListing(p, relPath, name)
		s := classifyEntry(p, relPath, listing, name)
		states[p] = s
		if s.IsLive {
			d := s.IsDir
			entryIsDir = &d
		}
	}

	if entryIsDir == nil {
		for _, p := range contributing {
			s := states[p]
			if s != nil && s.Classification != ClassNoOpinion {
				d := s.IsDir
				entryIsDir = &d
				break
			}
		}
	}

	// Check subordinates
	subListings := make(map[*SyncPeer]map[string]fsys.Entry)
	for _, p := range subordinates {
		listing := ws.gatherListing(p, relPath, name)
		subListings[p] = listing
		if entryIsDir == nil {
			if entry, ok := listing[name]; ok {
				d := entry.IsDir
				entryIsDir = &d
			}
		}
	}

	if entryIsDir == nil {
		return
	}

	if *entryIsDir {
		ws.handleDirEvent(relPath, name, dirPath, parentRelPath, contributing, subordinates, canon, reachable, states, subListings)
	} else {
		ws.handleFileEvent(relPath, name, dirPath, parentRelPath, contributing, subordinates, canon, reachable, states, subListings)
	}
}

func (ws *WatchSession) gatherListing(p *SyncPeer, relPath string, name string) map[string]fsys.Entry {
	listing := make(map[string]fsys.Entry)
	scheme := urlnorm.Scheme(p.ActiveURL)
	if scheme == "file" {
		// Local peer: stat() for live state
		entry, _ := p.ListingFS.Stat(relPath)
		if entry != nil {
			listing[name] = *entry
		}
	} else {
		// Remote peer: use snapshot state
		row, _ := p.Snapshot.Lookup(relPath)
		if row != nil && !row.DeletedTime.Valid {
			modTime, _ := ts.Parse(row.ModTime)
			listing[name] = fsys.Entry{
				Name:     name,
				IsDir:    row.ByteSize == -1,
				ModTime:  modTime,
				ByteSize: row.ByteSize,
			}
		}
	}
	return listing
}

func (ws *WatchSession) handleFileEvent(
	entryPath, name, dirPath, parentRelPath string,
	contributing, subordinates []*SyncPeer,
	canon *SyncPeer,
	active []*SyncPeer,
	states map[*SyncPeer]*PeerState,
	subListings map[*SyncPeer]map[string]fsys.Entry,
) {
	e := ws.eng
	decision := decideFile(canon, contributing, states)
	now := ts.Now()

	// Update snapshots for live entries
	for _, p := range contributing {
		s := states[p]
		if s == nil {
			continue
		}
		if s.IsLive {
			p.Snapshot.Upsert(entryPath, parentRelPath, name,
				ts.Format(s.ModTime), s.ByteSize, &now, nil)
		} else if s.Classification == ClassDeleted || s.Classification == ClassAbsentUnconfirmed {
			row, _ := p.Snapshot.Lookup(entryPath)
			if row != nil && !row.DeletedTime.Valid {
				ls := now
				if row.LastSeen.Valid {
					ls = row.LastSeen.String
				}
				p.Snapshot.SetDeletedTime(entryPath, ls)
			}
		}
	}

	switch decision.Action {
	case ActionPush:
		if decision.SrcPeer != nil {
			logged := false
			for _, dst := range decision.Targets {
				if !logged {
					logx.Info("W C %s", entryPath)
					logged = true
				}
				dst.Snapshot.Upsert(entryPath, parentRelPath, name,
					ts.Format(decision.SrcMod), decision.SrcSize, nil, nil)
				if !e.DryRun && ws.copyQueue != nil {
					ws.copyQueue.Enqueue(CopyJob{
						SrcPeer: decision.SrcPeer,
						DstPeer: dst,
						RelPath: entryPath,
						WinMod:  decision.SrcMod,
						WinSize: decision.SrcSize,
					})
				}
			}
			// Handle subordinates
			for _, sub := range subordinates {
				subEntry, subExists := subListings[sub][name]
				needsCopy := false
				if !subExists {
					needsCopy = true
				} else if !subEntry.IsDir {
					diff := decision.SrcMod.Sub(subEntry.ModTime)
					if diff < 0 {
						diff = -diff
					}
					if diff > TimeTolerance || subEntry.ByteSize != decision.SrcSize {
						needsCopy = true
					}
				}
				if needsCopy {
					if !logged {
						logx.Info("W C %s", entryPath)
						logged = true
					}
					sub.Snapshot.Upsert(entryPath, parentRelPath, name,
						ts.Format(decision.SrcMod), decision.SrcSize, nil, nil)
					if !e.DryRun && ws.copyQueue != nil {
						ws.copyQueue.Enqueue(CopyJob{
							SrcPeer: decision.SrcPeer,
							DstPeer: sub,
							RelPath: entryPath,
							WinMod:  decision.SrcMod,
							WinSize: decision.SrcSize,
						})
					}
				} else if subExists {
					sub.Snapshot.Upsert(entryPath, parentRelPath, name,
						ts.Format(subEntry.ModTime), subEntry.ByteSize, &now, nil)
				}
			}
		}

	case ActionDelete:
		logx.Info("W X %s", entryPath)
		for _, p := range active {
			entry, _ := p.ListingFS.Stat(entryPath)
			if entry != nil && !entry.IsDir {
				displaceEntry(p, entryPath, e.DryRun)
			}
			row, _ := p.Snapshot.Lookup(entryPath)
			if row != nil && !row.DeletedTime.Valid {
				ls := now
				if row.LastSeen.Valid {
					ls = row.LastSeen.String
				}
				p.Snapshot.SetDeletedTime(entryPath, ls)
			}
		}

	case ActionDeleteSubordinatesOnly:
		for _, sub := range subordinates {
			entry, _ := sub.ListingFS.Stat(entryPath)
			if entry != nil {
				logx.Info("W X %s", entryPath)
				displaceEntry(sub, entryPath, e.DryRun)
				row, _ := sub.Snapshot.Lookup(entryPath)
				if row != nil && !row.DeletedTime.Valid {
					ls := now
					if row.LastSeen.Valid {
						ls = row.LastSeen.String
					}
					sub.Snapshot.SetDeletedTime(entryPath, ls)
				}
			}
		}
	}
}

func (ws *WatchSession) handleDirEvent(
	entryPath, name, dirPath, parentRelPath string,
	contributing, subordinates []*SyncPeer,
	canon *SyncPeer,
	active []*SyncPeer,
	states map[*SyncPeer]*PeerState,
	subListings map[*SyncPeer]map[string]fsys.Entry,
) {
	e := ws.eng
	decision := decideDir(canon, contributing, states)
	now := ts.Now()

	if decision.Action == ActionPush {
		for _, p := range active {
			entry, _ := p.ListingFS.Stat(entryPath)
			if entry == nil || !entry.IsDir {
				if !e.DryRun {
					p.ListingFS.CreateDir(entryPath)
				}
			}
			p.Snapshot.Upsert(entryPath, parentRelPath, name,
				ts.Format(time.Now().UTC()), -1, &now, nil)
		}
	} else if decision.Action == ActionDelete {
		logx.Info("W X %s", entryPath)
		for _, p := range active {
			entry, _ := p.ListingFS.Stat(entryPath)
			if entry != nil && entry.IsDir {
				displaceEntry(p, entryPath, e.DryRun)
				cascadeTime := ts.Now()
				dirRow, _ := p.Snapshot.Lookup(entryPath)
				if dirRow != nil && dirRow.LastSeen.Valid {
					cascadeTime = dirRow.LastSeen.String
				}
				p.Snapshot.CascadeTombstones(entryPath, cascadeTime)
			}
		}
	}
}

// WaitForPendingCopies waits for any in-progress copies to finish.
func (ws *WatchSession) WaitForPendingCopies(timeout time.Duration) {
	if ws.copyQueue == nil {
		return
	}
	// Close the queue and wait for pending copies
	ws.copyQueue.CloseAndWait(timeout)
	// Reopen a fresh queue for new events
	ws.copyQueue = NewCopyQueue(ws.eng.Opts.MC*2, ws.eng.checkCopyCheckpoint)
}

func init() {
	_ = strings.TrimSpace
	_ = (*sync.Mutex)(nil)
}
