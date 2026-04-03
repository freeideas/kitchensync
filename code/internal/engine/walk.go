package engine

import (
	"io"
	"kitchensync/internal/args"
	"kitchensync/internal/db"
	"kitchensync/internal/fsys"
	"kitchensync/internal/ignore"
	"kitchensync/internal/logx"
	"kitchensync/internal/ts"
	"sort"
	"strings"
	"sync"
	"time"
)

type Engine struct {
	Opts      args.Options
	Peers     []*SyncPeer
	CopyQueue *CopyQueue
	DryRun    bool

	checkpointMu   sync.Mutex
	lastCheckpoint  time.Time
}

func NewEngine(opts args.Options, peers []*SyncPeer) *Engine {
	e := &Engine{
		Opts:           opts,
		Peers:          peers,
		DryRun:         opts.DryRun,
		lastCheckpoint: time.Now(),
	}
	if !opts.DryRun {
		e.CopyQueue = NewCopyQueue(opts.MC*2, e.checkCopyCheckpoint)
	}
	return e
}

func (e *Engine) checkCopyCheckpoint() {
	e.checkpointMu.Lock()
	defer e.checkpointMu.Unlock()
	if time.Since(e.lastCheckpoint) > time.Duration(e.Opts.SI)*time.Minute {
		e.uploadSnapshots()
		e.lastCheckpoint = time.Now()
	}
}

func (e *Engine) Run() {
	reachable := e.reachablePeers()
	rootRules := ignore.NewRules()
	e.syncDirectory(reachable, "", rootRules)

	if e.CopyQueue != nil {
		e.CopyQueue.CloseAndWait(60 * time.Second)
	}
}

func (e *Engine) reachablePeers() []*SyncPeer {
	var peers []*SyncPeer
	for _, p := range e.Peers {
		if p.Reachable {
			peers = append(peers, p)
		}
	}
	return peers
}

func (e *Engine) contributingPeers(peers []*SyncPeer) []*SyncPeer {
	var result []*SyncPeer
	for _, p := range peers {
		if !p.IsSub() {
			result = append(result, p)
		}
	}
	return result
}

func (e *Engine) subordinatePeers(peers []*SyncPeer) []*SyncPeer {
	var result []*SyncPeer
	for _, p := range peers {
		if p.IsSub() {
			result = append(result, p)
		}
	}
	return result
}

func (e *Engine) canonPeer(peers []*SyncPeer) *SyncPeer {
	for _, p := range peers {
		if p.IsCanon() {
			return p
		}
	}
	return nil
}

func (e *Engine) syncDirectory(peers []*SyncPeer, dirPath string, parentRules *ignore.Rules) {
	// Phase 1: List all peers in parallel
	type listResult struct {
		peer    *SyncPeer
		entries map[string]fsys.Entry
		err     error
	}

	results := make([]listResult, len(peers))
	var wg sync.WaitGroup
	for i, p := range peers {
		wg.Add(1)
		go func(idx int, peer *SyncPeer) {
			defer wg.Done()
			entries, err := peer.ListingFS.ListDir(dirPath)
			if err != nil {
				results[idx] = listResult{peer: peer, err: err}
				return
			}
			m := make(map[string]fsys.Entry)
			for _, e := range entries {
				m[e.Name] = e
			}
			results[idx] = listResult{peer: peer, entries: m}
		}(i, p)
	}
	wg.Wait()

	// Drop peers with errors
	var active []*SyncPeer
	listings := make(map[*SyncPeer]map[string]fsys.Entry)
	for _, r := range results {
		if r.err != nil {
			logx.Error("listing failed for %s at %s: %v", r.peer.ActiveURL, dirPath, r.err)
		} else {
			active = append(active, r.peer)
			listings[r.peer] = r.entries
		}
	}

	contributing := e.contributingPeers(active)
	subordinates := e.subordinatePeers(active)

	if len(contributing) == 0 {
		logx.Warn("no contributing peer available at %s, skipping subtree", dirPath)
		return
	}

	// Phase 2: Union entry names
	nameSet := make(map[string]bool)
	for _, p := range contributing {
		for name := range listings[p] {
			nameSet[name] = true
		}
	}
	for _, p := range subordinates {
		for name := range listings[p] {
			nameSet[name] = true
		}
	}

	// Phase 2b: Resolve .syncignore first
	rules := parentRules
	if nameSet[".syncignore"] {
		// Read winning .syncignore (newest among contributing peers)
		content := e.readWinningSyncIgnore(contributing, listings, dirPath)
		if content != "" {
			rules = parentRules.Merge(content)
		}
		delete(nameSet, ".syncignore")
	}

	// Sort names for deterministic order
	var names []string
	for name := range nameSet {
		names = append(names, name)
	}
	sort.Strings(names)

	// Filter by ignore rules
	var filtered []string
	for _, name := range names {
		isDir := false
		for _, p := range active {
			if e, ok := listings[p][name]; ok {
				isDir = e.IsDir
				break
			}
		}
		if !rules.Matches(name, isDir) {
			filtered = append(filtered, name)
		}
	}

	// Phase 3: Decide and act on each entry
	type recurseItem struct {
		peers   []*SyncPeer
		subPath string
	}
	var dirsToRecurse []recurseItem

	canon := e.canonPeer(active)

	for _, name := range filtered {
		entryPath := db.RelPath(dirPath, name)
		parentRelPath := dirPath
		if parentRelPath == "" {
			parentRelPath = db.SentinelPath
		}

		// Gather states from contributing peers
		states := make(map[*SyncPeer]*PeerState)
		var entryIsDir *bool
		for _, p := range contributing {
			s := classifyEntry(p, entryPath, listings[p], name)
			states[p] = s
			if s.IsLive {
				d := s.IsDir
				entryIsDir = &d
			}
		}

		// Determine entry type
		if entryIsDir == nil {
			// Check snapshots for type info
			for _, p := range contributing {
				s := states[p]
				if s.Classification != ClassNoOpinion {
					d := s.IsDir
					entryIsDir = &d
					break
				}
			}
		}
		if entryIsDir == nil {
			// Check subordinates
			for _, p := range subordinates {
				if e, ok := listings[p][name]; ok {
					d := e.IsDir
					entryIsDir = &d
					break
				}
			}
		}
		if entryIsDir == nil {
			continue
		}

		// Type conflict resolution
		hasFile := false
		hasDir := false
		for _, s := range states {
			if s.Classification == ClassNoOpinion {
				continue
			}
			if s.IsDir {
				hasDir = true
			} else {
				hasFile = true
			}
		}
		if hasFile && hasDir {
			if canon != nil {
				cs := states[canon]
				if cs != nil && cs.IsLive {
					*entryIsDir = cs.IsDir
				}
			} else {
				*entryIsDir = false // file wins
			}
		}

		if *entryIsDir {
			decision := decideDir(canon, contributing, states)

			recursionPeers := []*SyncPeer{}
			for _, peer := range active {
				listing := listings[peer]
				entry, exists := listing[name]

				// Wrong type: displace
				if exists && !entry.IsDir {
					logx.Info("X %s", entryPath)
					if err := displaceEntry(peer, entryPath, e.DryRun); err != nil {
						logx.Error("displace failed %s on %s: %v", entryPath, peer.ActiveURL, err)
						continue
					}
				}

				if decision.Action == ActionDelete && exists && entry.IsDir {
					logx.Info("X %s", entryPath)
					if err := displaceEntry(peer, entryPath, e.DryRun); err != nil {
						logx.Error("displace dir failed %s on %s: %v", entryPath, peer.ActiveURL, err)
						continue
					}
					// Cascade tombstones
					now := ts.Now()
					peer.Snapshot.CascadeTombstones(entryPath, now)
					continue
				}

				if decision.Action == ActionPush && (!exists || (exists && !entry.IsDir)) {
					if !e.DryRun {
						peer.ListingFS.CreateDir(entryPath)
					}
					now := ts.Now()
					peer.Snapshot.Upsert(entryPath, parentRelPath, name, ts.Format(time.Now().UTC()), -1, &now, nil)
					recursionPeers = append(recursionPeers, peer)
				} else if exists && entry.IsDir {
					recursionPeers = append(recursionPeers, peer)
				}
			}

			// Update snapshot for present dirs
			now := ts.Now()
			for _, p := range active {
				entry, exists := listings[p][name]
				if exists && entry.IsDir {
					p.Snapshot.Upsert(entryPath, parentRelPath, name,
						ts.Format(entry.ModTime), -1, &now, nil)
				} else if !exists {
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

			if len(recursionPeers) > 0 && decision.Action != ActionDelete {
				dirsToRecurse = append(dirsToRecurse, recurseItem{peers: recursionPeers, subPath: entryPath})
			}
		} else {
			// File decision
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
					logx.Info("C %s", entryPath)
					// Enqueue copies to contributing targets
					for _, dst := range decision.Targets {
						// Displace if dst has directory
						if entry, ok := listings[dst][name]; ok && entry.IsDir {
							displaceEntry(dst, entryPath, e.DryRun)
						}
						// Pre-update snapshot for dst
						dst.Snapshot.Upsert(entryPath, parentRelPath, name,
							ts.Format(decision.SrcMod), decision.SrcSize, nil, nil)
						if !e.DryRun && e.CopyQueue != nil {
							e.CopyQueue.Enqueue(CopyJob{
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
						subEntry, subExists := listings[sub][name]
						if subExists && subEntry.IsDir {
							displaceEntry(sub, entryPath, e.DryRun)
						}
						needsCopy := false
						if !subExists {
							needsCopy = true
						} else if subExists && !subEntry.IsDir {
							diff := decision.SrcMod.Sub(subEntry.ModTime)
							if diff < 0 {
								diff = -diff
							}
							if diff > TimeTolerance || subEntry.ByteSize != decision.SrcSize {
								needsCopy = true
							}
						}
						if needsCopy {
							sub.Snapshot.Upsert(entryPath, parentRelPath, name,
								ts.Format(decision.SrcMod), decision.SrcSize, nil, nil)
							if !e.DryRun && e.CopyQueue != nil {
								e.CopyQueue.Enqueue(CopyJob{
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
				logx.Info("X %s", entryPath)
				for _, p := range active {
					if entry, ok := listings[p][name]; ok {
						if !entry.IsDir {
							displaceEntry(p, entryPath, e.DryRun)
						}
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
					if _, ok := listings[sub][name]; ok {
						logx.Info("X %s", entryPath)
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
	}

	// Phase 4: BAK/TMP cleanup at this level
	if !e.DryRun {
		for _, peer := range active {
			var ksDir string
			if dirPath == "" {
				ksDir = ".kitchensync"
			} else {
				ksDir = dirPath + "/.kitchensync"
			}
			entry, _ := peer.ListingFS.Stat(ksDir)
			if entry != nil && entry.IsDir {
				if e.Opts.BD > 0 {
					e.cleanupExpired(peer, ksDir+"/BAK", e.Opts.BD)
				}
				if e.Opts.XD > 0 {
					e.cleanupExpired(peer, ksDir+"/TMP", e.Opts.XD)
				}
			}
		}
	}

	// Phase 5: Recurse into subdirectories
	for _, item := range dirsToRecurse {
		e.syncDirectory(item.peers, item.subPath, rules)
	}
}

func (e *Engine) readWinningSyncIgnore(contributing []*SyncPeer, listings map[*SyncPeer]map[string]fsys.Entry, dirPath string) string {
	// Find the winning peer for .syncignore (newest mod_time among contributing)
	var bestPeer *SyncPeer
	var bestMod time.Time
	for _, p := range contributing {
		entry, ok := listings[p][".syncignore"]
		if ok && !entry.IsDir && entry.ModTime.After(bestMod) {
			bestMod = entry.ModTime
			bestPeer = p
		}
	}
	if bestPeer == nil {
		return ""
	}

	syncIgnorePath := db.RelPath(dirPath, ".syncignore")
	reader, err := bestPeer.ListingFS.ReadFile(syncIgnorePath)
	if err != nil {
		logx.Warn("failed to read .syncignore from %s: %v", bestPeer.ActiveURL, err)
		return ""
	}
	defer reader.Close()
	data, err := io.ReadAll(reader)
	if err != nil {
		logx.Warn("failed to read .syncignore: %v", err)
		return ""
	}
	return string(data)
}

func (e *Engine) cleanupExpired(peer *SyncPeer, dirPath string, maxDays int) {
	entries, err := peer.ListingFS.ListDir(dirPath)
	if err != nil {
		return
	}
	cutoff := time.Now().UTC().AddDate(0, 0, -maxDays)
	for _, entry := range entries {
		if !entry.IsDir {
			continue
		}
		// Parse timestamp from directory name
		t, err := ts.Parse(entry.Name)
		if err != nil {
			continue
		}
		if t.Before(cutoff) {
			// Remove entire timestamp directory
			subPath := dirPath + "/" + entry.Name
			e.removeAll(peer, subPath)
		}
	}
}

func (e *Engine) removeAll(peer *SyncPeer, dirPath string) {
	entries, err := peer.ListingFS.ListDir(dirPath)
	if err != nil {
		return
	}
	for _, entry := range entries {
		subPath := dirPath + "/" + entry.Name
		if entry.IsDir {
			e.removeAll(peer, subPath)
		} else {
			peer.ListingFS.DeleteFile(subPath)
		}
	}
	peer.ListingFS.DeleteDir(dirPath)
}

func (e *Engine) uploadSnapshots() {
	for _, peer := range e.Peers {
		if !peer.Reachable {
			continue
		}
		if err := e.uploadSnapshot(peer); err != nil {
			logx.Error("snapshot upload failed for %s: %v", peer.ActiveURL, err)
		}
	}
}

func (e *Engine) UploadSnapshots() {
	e.uploadSnapshots()
}

func (e *Engine) uploadSnapshot(peer *SyncPeer) error {
	// WAL checkpoint
	if err := peer.Snapshot.Checkpoint(); err != nil {
		return err
	}

	// Read the db file
	dbPath := peer.Snapshot.Path()
	reader, err := (&fsys.LocalFS{}).ReadFile(dbPath)
	if err != nil {
		// Try reading directly
		return err
	}

	timestamp := ts.Now()
	tmpPath := ".kitchensync/TMP/" + timestamp + "/snapshot.db"
	if err := peer.ListingFS.WriteFile(tmpPath, reader); err != nil {
		reader.Close()
		return err
	}
	reader.Close()

	// Atomic rename
	if err := peer.ListingFS.Rename(tmpPath, ".kitchensync/snapshot.db"); err != nil {
		return err
	}

	return nil
}

func init() {
	_ = strings.TrimSpace
	_ = sort.Strings
}
