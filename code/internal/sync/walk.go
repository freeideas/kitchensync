package sync

import (
	"fmt"
	"io"
	"path"
	"strconv"
	"strings"
	gosync "sync"
	"time"

	"kitchensync/internal/cli"
	"kitchensync/internal/fsutil"
	"kitchensync/internal/ignore"
	"kitchensync/internal/log"
	"kitchensync/internal/peer"
	"kitchensync/internal/snapshot"
	"kitchensync/internal/timestamp"
)

type recurseItem struct {
	peers   []*peer.Peer
	subpath string
}

// CopyJob represents a file copy to be executed concurrently.
type CopyJob struct {
	Src     *peer.Peer
	Dst     *peer.Peer
	RelPath string
	ModTime string
	Size    int64
}

// Engine holds the sync state.
type Engine struct {
	Opts      cli.Options
	Peers     []*peer.Peer
	Canon     *peer.Peer
	CopyJobs  chan CopyJob
	WG        gosync.WaitGroup
	copyLogMu gosync.Mutex
	logged    map[string]bool // track logged paths to log once per decision
}

// NewEngine creates a sync engine.
func NewEngine(opts cli.Options, peers []*peer.Peer, canon *peer.Peer) *Engine {
	e := &Engine{
		Opts:     opts,
		Peers:    peers,
		Canon:    canon,
		CopyJobs: make(chan CopyJob, 100),
		logged:   make(map[string]bool),
	}
	// Start copy workers
	for i := 0; i < opts.MC*2; i++ {
		e.WG.Add(1)
		go e.copyWorker()
	}
	return e
}

func (e *Engine) copyWorker() {
	defer e.WG.Done()
	for job := range e.CopyJobs {
		if err := CopyFile(job.Src, job.Dst, job.RelPath, job.ModTime, job.Size); err != nil {
			log.Error("copy %s from %s to %s: %v", job.RelPath, job.Src.Label(), job.Dst.Label(), err)
		}
	}
}

// Wait waits for all copy jobs to finish.
func (e *Engine) Wait() {
	close(e.CopyJobs)
	e.WG.Wait()
}

func (e *Engine) logCopy(relPath string) {
	e.copyLogMu.Lock()
	defer e.copyLogMu.Unlock()
	if !e.logged[relPath] {
		log.Info("C %s", relPath)
		e.logged[relPath] = true
	}
}

func (e *Engine) logDelete(relPath string) {
	e.copyLogMu.Lock()
	defer e.copyLogMu.Unlock()
	key := "X:" + relPath
	if !e.logged[key] {
		log.Info("X %s", relPath)
		e.logged[key] = true
	}
}

// SyncDirectory performs the combined-tree walk at a directory level.
func (e *Engine) SyncDirectory(peers []*peer.Peer, dirPath string, parentRules *ignore.Rules) {
	// Phase 1: List all peers in parallel
	type listResult struct {
		peer    *peer.Peer
		entries map[string]fsutil.DirEntry
		err     error
	}

	results := make([]listResult, len(peers))
	var wg gosync.WaitGroup
	for i, p := range peers {
		wg.Add(1)
		go func(idx int, pr *peer.Peer) {
			defer wg.Done()
			entries, err := pr.ListConn.ListDir(dirPath)
			if err != nil {
				results[idx] = listResult{peer: pr, err: err}
				return
			}
			m := make(map[string]fsutil.DirEntry, len(entries))
			for _, e := range entries {
				m[e.Name] = e
			}
			results[idx] = listResult{peer: pr, entries: m}
		}(i, p)
	}
	wg.Wait()

	// Drop peers with listing errors
	var active []*peer.Peer
	listings := make(map[*peer.Peer]map[string]fsutil.DirEntry)
	for _, r := range results {
		if r.err != nil {
			log.Error("listing failed for %s at %s, excluding from subtree: %v", r.peer.Label(), dirPath, r.err)
			continue
		}
		active = append(active, r.peer)
		listings[r.peer] = r.entries
	}

	// Phase 2: Union entry names
	var contributing []*peer.Peer
	var subordinates []*peer.Peer
	for _, p := range active {
		if p.IsSubordinate() {
			subordinates = append(subordinates, p)
		} else {
			contributing = append(contributing, p)
		}
	}

	nameSet := make(map[string]bool)
	for _, p := range active {
		for name := range listings[p] {
			nameSet[name] = true
		}
	}

	// Phase 2b: Resolve .syncignore first
	rules := parentRules
	if rules == nil {
		rules = ignore.DefaultRules()
	}

	if nameSet[".syncignore"] {
		// Decide winning .syncignore using normal rules
		syncIgnorePath := joinPath(dirPath, ".syncignore")
		e.handleFileEntry(syncIgnorePath, ".syncignore", active, contributing, subordinates, listings)

		// Read winning .syncignore content
		content := e.readWinningSyncIgnore(syncIgnorePath, contributing, subordinates, listings)
		if content != "" {
			rules = rules.Merge(content)
		}
		delete(nameSet, ".syncignore")
	}

	// Collect all names, filter by ignore rules
	var names []string
	for name := range nameSet {
		if ignore.IsBuiltinExclude(name) {
			continue
		}
		// Check if entry is a directory across any peer
		entryIsDir := false
		for _, p := range active {
			if e, ok := listings[p][name]; ok && e.IsDir {
				entryIsDir = true
				break
			}
		}
		if entryIsDir {
			if rules.MatchesDir(name) {
				continue
			}
		} else {
			if rules.Matches(name) {
				continue
			}
		}
		names = append(names, name)
	}

	// Phase 3: Decide and act on each entry (pre-order)
	var dirsToRecurse []recurseItem

	for _, name := range names {
		entryPath := joinPath(dirPath, name)

		// Determine if this is a file or directory
		isDir := false
		isFile := false
		for _, p := range active {
			if e, ok := listings[p][name]; ok {
				if e.IsDir {
					isDir = true
				} else {
					isFile = true
				}
			}
		}

		if isDir && !isFile {
			// Pure directory
			recursionPeers := e.handleDirEntry(entryPath, name, active, contributing, subordinates, listings)
			if len(recursionPeers) > 0 {
				dirsToRecurse = append(dirsToRecurse, recurseItem{peers: recursionPeers, subpath: entryPath})
			}
		} else if isFile && !isDir {
			// Pure file
			e.handleFileEntry(entryPath, name, active, contributing, subordinates, listings)
		} else {
			// Type conflict: file on some, directory on others
			e.handleTypeConflict(entryPath, name, active, contributing, subordinates, listings, &dirsToRecurse)
		}
	}

	// Phase 4: BAK/TMP cleanup at this level
	for _, p := range active {
		ksDir := joinPath(dirPath, ".kitchensync")
		if exists, _ := p.ListConn.Exists(ksDir); exists {
			if e.Opts.BD > 0 {
				cleanupExpired(p.ListConn, joinPath(ksDir, "BAK"), e.Opts.BD)
			}
			if e.Opts.XD > 0 {
				cleanupExpired(p.ListConn, joinPath(ksDir, "TMP"), e.Opts.XD)
			}
		}
	}

	// Phase 5: Recurse into subdirectories
	for _, item := range dirsToRecurse {
		e.SyncDirectory(item.peers, item.subpath, rules)
	}
}

func (e *Engine) handleDirEntry(entryPath, name string, active, contributing, subordinates []*peer.Peer, listings map[*peer.Peer]map[string]fsutil.DirEntry) []*peer.Peer {
	// Gather states from contributing peers
	states := make(map[*peer.Peer]PeerState)
	for _, p := range contributing {
		states[p] = ClassifyDirEntry(p, entryPath, listings[p])
	}

	decision := DecideDir(states, e.Canon)
	nowStr := snapshot.NowStr()
	parentPath := snapshot.ParentPath(entryPath)

	var recursionPeers []*peer.Peer

	for _, p := range active {
		entry, hasEntry := listings[p][name]

		// Type conflict: peer has file where dir should be
		if hasEntry && !entry.IsDir {
			if err := Displace(p, entryPath); err != nil {
				continue
			}
			MarkDeleted(p, entryPath)
			hasEntry = false
		}

		if decision.Act == ActionDelete || decision.Act == ActionDeleteSubordinatesOnly {
			if hasEntry {
				if decision.Act == ActionDelete || p.IsSubordinate() {
					if err := Displace(p, entryPath); err != nil {
						// Displacement failed: exclude from recursion, don't cascade
						continue
					}
					CascadeTombstones(p, entryPath)
					e.logDelete(entryPath)
				} else {
					recursionPeers = append(recursionPeers, p)
				}
			}
		} else {
			// Create or keep
			if !hasEntry {
				if err := p.ListConn.CreateDir(entryPath); err != nil {
					log.Error("create dir %s on %s: %v", entryPath, p.Label(), err)
					continue
				}
				p.Snap.Upsert(entryPath, parentPath, name, "0000-00-00_00-00-00_000000Z", -1, &nowStr, nil)
			} else {
				// Update snapshot: dir exists
				p.Snap.Upsert(entryPath, parentPath, name, "0000-00-00_00-00-00_000000Z", -1, &nowStr, nil)
			}
			recursionPeers = append(recursionPeers, p)
		}
	}

	return recursionPeers
}

func (e *Engine) handleFileEntry(entryPath, name string, active, contributing, subordinates []*peer.Peer, listings map[*peer.Peer]map[string]fsutil.DirEntry) {
	// Gather states from contributing peers
	contribStates := make(map[*peer.Peer]PeerState)
	for _, p := range contributing {
		contribStates[p] = ClassifyEntry(p, entryPath, listings[p])
	}

	// Include all peers in allStates for target computation
	allStates := make(map[*peer.Peer]PeerState)
	for p, s := range contribStates {
		allStates[p] = s
	}
	for _, p := range subordinates {
		allStates[p] = ClassifyEntry(p, entryPath, listings[p])
	}

	decision := Decide(contribStates, e.Canon)
	nowStr := snapshot.NowStr()
	parentPath := snapshot.ParentPath(entryPath)

	switch decision.Act {
	case ActionNone:
		// Find a source peer for subordinate conformance (any contributing peer with the file)
		var conformanceSrc *peer.Peer
		var conformanceModStr string
		var conformanceSize int64
		for _, p := range contributing {
			if entry, has := listings[p][name]; has && !entry.IsDir {
				conformanceSrc = p
				conformanceModStr = snapshot.FormatModTime(entry.ModTime)
				conformanceSize = entry.ByteSize
				break
			}
		}

		// Update snapshots for all peers that have the file
		for _, p := range active {
			entry, has := listings[p][name]

			// Subordinate conformance: push to subordinates that are missing or differ
			if p.IsSubordinate() && conformanceSrc != nil {
				needsCopy := false
				if !has {
					needsCopy = true
				} else if !entry.IsDir {
					if !withinTolerance(entry.ModTime, decision.ModTime) || entry.ByteSize != decision.Size {
						needsCopy = true
					}
				}
				if needsCopy {
					e.logCopy(entryPath)
					p.Snap.UpsertWithNullLastSeen(entryPath, parentPath, name, conformanceModStr, conformanceSize)
					e.CopyJobs <- CopyJob{
						Src:     conformanceSrc,
						Dst:     p,
						RelPath: entryPath,
						ModTime: conformanceModStr,
						Size:    conformanceSize,
					}
					continue
				}
			}

			if has && !entry.IsDir {
				modStr := snapshot.FormatModTime(entry.ModTime)
				p.Snap.Upsert(entryPath, parentPath, name, modStr, entry.ByteSize, &nowStr, nil)
			} else if has && entry.IsDir {
				// Type conflict handled elsewhere
			} else {
				// Absent: mark as deleted if row exists
				MarkDeleted(p, entryPath)
			}
		}

	case ActionPush:
		if decision.Src == nil {
			return
		}
		e.logCopy(entryPath)

		modStr := snapshot.FormatModTime(decision.ModTime)

		// Update source snapshot
		decision.Src.Snap.Upsert(entryPath, parentPath, name, modStr, decision.Size, &nowStr, nil)

		// Update all peers
		for _, p := range active {
			if p == decision.Src {
				continue
			}

			entry, has := listings[p][name]

			// Check if this peer needs the file
			needsCopy := false
			for _, t := range decision.Targets {
				if t == p {
					needsCopy = true
					break
				}
			}

			// Also check subordinates
			if p.IsSubordinate() && !has {
				needsCopy = true
			} else if p.IsSubordinate() && has && !entry.IsDir {
				if !withinTolerance(entry.ModTime, decision.ModTime) || entry.ByteSize != decision.Size {
					needsCopy = true
				}
			}

			if needsCopy {
				// Handle type conflict: peer has dir where file should be
				if has && entry.IsDir {
					if err := Displace(p, entryPath); err != nil {
						continue
					}
				}

				// Upsert with NULL last_seen (pending copy)
				p.Snap.UpsertWithNullLastSeen(entryPath, parentPath, name, modStr, decision.Size)

				e.CopyJobs <- CopyJob{
					Src:     decision.Src,
					Dst:     p,
					RelPath: entryPath,
					ModTime: modStr,
					Size:    decision.Size,
				}
			} else if has && !entry.IsDir {
				// Peer already has matching file
				p.Snap.Upsert(entryPath, parentPath, name, modStr, decision.Size, &nowStr, nil)
			} else {
				MarkDeleted(p, entryPath)
			}
		}

	case ActionDelete, ActionDeleteSubordinatesOnly:
		e.logDelete(entryPath)
		for _, p := range active {
			entry, has := listings[p][name]
			if has && !entry.IsDir {
				if decision.Act == ActionDelete || p.IsSubordinate() {
					if err := Displace(p, entryPath); err != nil {
						continue
					}
				}
			}
			MarkDeleted(p, entryPath)
		}
	}
}

func (e *Engine) handleTypeConflict(entryPath, name string, active, contributing, subordinates []*peer.Peer, listings map[*peer.Peer]map[string]fsutil.DirEntry, dirsToRecurse *[]recurseItem) {
	// Type conflict: same path is file on some peers, directory on others
	// Canon's type wins; no canon -> file wins
	winnerIsDir := false
	if e.Canon != nil {
		if entry, ok := listings[e.Canon][name]; ok {
			winnerIsDir = entry.IsDir
		}
	}

	if winnerIsDir {
		// Directory wins: displace files, handle as dir
		for _, p := range active {
			if entry, ok := listings[p][name]; ok && !entry.IsDir {
				Displace(p, entryPath)
				MarkDeleted(p, entryPath)
				delete(listings[p], name)
			}
		}
		recursionPeers := e.handleDirEntry(entryPath, name, active, contributing, subordinates, listings)
		if len(recursionPeers) > 0 {
			*dirsToRecurse = append(*dirsToRecurse, recurseItem{peers: recursionPeers, subpath: entryPath})
		}
	} else {
		// File wins: displace directories, handle as file
		for _, p := range active {
			if entry, ok := listings[p][name]; ok && entry.IsDir {
				Displace(p, entryPath)
				CascadeTombstones(p, entryPath)
				delete(listings[p], name)
			}
		}
		e.handleFileEntry(entryPath, name, active, contributing, subordinates, listings)
	}
}

func (e *Engine) readWinningSyncIgnore(relPath string, contributing, subordinates []*peer.Peer, listings map[*peer.Peer]map[string]fsutil.DirEntry) string {
	// Find the peer that has the winning .syncignore (newest mod_time)
	var winnerPeer *peer.Peer
	var winnerTime time.Time

	allPeers := append(contributing, subordinates...)
	for _, p := range allPeers {
		name := snapshot.BaseName(relPath)
		if entry, ok := listings[p][name]; ok && !entry.IsDir {
			if entry.ModTime.After(winnerTime) {
				winnerTime = entry.ModTime
				winnerPeer = p
			}
		}
	}

	if winnerPeer == nil {
		return ""
	}

	reader, err := winnerPeer.ListConn.ReadFile(relPath)
	if err != nil {
		return ""
	}
	defer reader.Close()
	data, err := io.ReadAll(reader)
	if err != nil {
		return ""
	}
	return string(data)
}


func joinPath(dir, name string) string {
	if dir == "" || dir == "." {
		return name
	}
	return dir + "/" + name
}

// cleanupExpired removes timestamp directories older than maxAge days.
func cleanupExpired(fs fsutil.PeerFS, dirPath string, maxAgeDays int) {
	entries, err := fs.ListDir(dirPath)
	if err != nil {
		return
	}
	cutoff := time.Now().UTC().AddDate(0, 0, -maxAgeDays)

	for _, entry := range entries {
		if !entry.IsDir {
			continue
		}
		// Parse timestamp from directory name
		t, err := timestamp.ParseTime(entry.Name)
		if err != nil {
			continue
		}
		if t.Before(cutoff) {
			// Remove the entire subtree
			removeRecursive(fs, path.Join(dirPath, entry.Name))
		}
	}
}

func removeRecursive(fs fsutil.PeerFS, dirPath string) {
	entries, err := fs.ListDir(dirPath)
	if err != nil {
		fs.DeleteDir(dirPath)
		return
	}
	for _, e := range entries {
		p := path.Join(dirPath, e.Name)
		if e.IsDir {
			removeRecursive(fs, p)
		} else {
			fs.DeleteFile(p)
		}
	}
	fs.DeleteDir(dirPath)
}

// ParseTimestampDir tries to parse a directory name as a KitchenSync timestamp.
func ParseTimestampDir(name string) (time.Time, error) {
	return timestamp.ParseTime(name)
}

// FormatInt converts an int to string for display.
func FormatInt(n int) string {
	return strconv.Itoa(n)
}

// Unused but satisfies potential import
var _ = fmt.Sprintf
var _ = strings.TrimSpace
