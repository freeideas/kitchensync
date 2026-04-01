package main

import (
	"fmt"
	"io"
	"os"
	"path"
	"time"

	"kitchensync/internal/cli"
	"kitchensync/internal/fsutil"
	"kitchensync/internal/log"
	"kitchensync/internal/peer"
	"kitchensync/internal/pool"
	"kitchensync/internal/snapshot"
	ksync "kitchensync/internal/sync"
	"kitchensync/internal/timestamp"
	"kitchensync/internal/urlutil"
)

func main() {
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	opts, peerArgs, help, err := cli.Parse(args)
	if err != nil {
		fmt.Println(err)
		fmt.Println()
		fmt.Println(cli.HelpText)
		return 1
	}
	if help {
		fmt.Println(cli.HelpText)
		return 0
	}
	if len(peerArgs) == 0 {
		fmt.Println("Error: at least one peer URL is required")
		fmt.Println()
		fmt.Println(cli.HelpText)
		return 1
	}

	log.SetLevel(opts.VL)

	// Build peer list
	peers := make([]*peer.Peer, len(peerArgs))
	for i, pa := range peerArgs {
		peers[i] = &peer.Peer{Arg: pa}
	}

	// Find canon peer
	var canonPeer *peer.Peer
	for _, p := range peers {
		if p.IsCanon() {
			canonPeer = p
			break
		}
	}

	// Connect to all peers in parallel
	connectPeers(peers, opts)

	// Check canon reachability
	if canonPeer != nil && !canonPeer.Reachable {
		fmt.Println("canon peer unreachable")
		return 1
	}

	// Count reachable
	var reachable []*peer.Peer
	for _, p := range peers {
		if p.Reachable {
			reachable = append(reachable, p)
		}
	}

	if len(peers) >= 2 && len(reachable) < 2 {
		fmt.Println("fewer than two peers reachable")
		return 1
	}
	if len(reachable) < 1 {
		fmt.Println("peer unreachable")
		return 1
	}

	// Download snapshots
	for _, p := range reachable {
		if err := downloadSnapshot(p); err != nil {
			log.Error("download snapshot for %s: %v", p.Label(), err)
			p.AutoSubordinate = true
		}
	}

	// Check: no snapshots and no canon in multi-peer mode
	if len(peers) >= 2 {
		anyHasData := false
		for _, p := range reachable {
			if p.Snap != nil {
				has, _ := p.Snap.HasData()
				if has {
					anyHasData = true
					break
				}
			}
		}
		if !anyHasData && canonPeer == nil {
			fmt.Println("First sync? Mark the authoritative peer with a leading +")
			return 1
		}

		// Check: at least one contributing peer reachable
		anyContributing := false
		for _, p := range reachable {
			if !p.IsSubordinate() {
				anyContributing = true
				break
			}
		}
		if !anyContributing {
			fmt.Println("No contributing peer reachable — cannot make sync decisions")
			return 1
		}
	}

	// Purge old tombstones
	if opts.TD > 0 {
		for _, p := range reachable {
			if p.Snap != nil {
				p.Snap.PurgeTombstones(opts.TD)
			}
		}
	}

	// Single-peer mode: snapshot only, no sync
	if len(reachable) == 1 {
		p := reachable[0]
		singlePeerSnapshot(p, "")
		// BAK/TMP cleanup (same as Phase 4 in the multi-peer walk)
		if !opts.DryRun {
			singlePeerCleanup(p, "", opts)
		}
		// Upload updated snapshot
		if !opts.DryRun && p.Snap != nil {
			if err := uploadSnapshot(p); err != nil {
				log.Error("upload snapshot for %s: %v", p.Label(), err)
			}
		}
		// Close connections
		if p.ListConn != nil {
			p.ListConn.Close()
		}
		log.Info("done (snapshot only)")
		return 0
	}

	// Create connection pools for reachable peers
	if !opts.DryRun {
		for _, p := range reachable {
			p.Pool = pool.NewPool(p.ActiveURL, opts.MC, opts.CT)
		}
	}

	// Run the walk
	engine := ksync.NewEngine(opts, reachable, canonPeer)
	engine.SyncDirectory(reachable, "", nil)

	// Wait for all copies
	engine.Wait()

	// Upload updated snapshots (skip in dry-run)
	if !opts.DryRun {
		for _, p := range reachable {
			if p.Snap != nil {
				if err := uploadSnapshot(p); err != nil {
					log.Error("upload snapshot for %s: %v", p.Label(), err)
				}
			}
		}
	}

	// Close connections
	for _, p := range reachable {
		if p.ListConn != nil {
			p.ListConn.Close()
		}
		if p.Pool != nil {
			p.Pool.Close()
		}
	}

	log.Info("done")
	return 0
}

func connectPeers(peers []*peer.Peer, opts cli.Options) {
	type result struct {
		idx  int
		url  *urlutil.NormalizedURL
		conn fsutil.PeerFS
	}
	ch := make(chan result, len(peers))

	for i, p := range peers {
		go func(idx int, pr *peer.Peer) {
			for _, u := range pr.Arg.URLs {
				conn := tryConnect(u, opts)
				if conn != nil {
					ch <- result{idx: idx, url: u, conn: conn}
					return
				}
			}
			ch <- result{idx: idx}
		}(i, p)
	}

	for range peers {
		r := <-ch
		if r.conn != nil {
			peers[r.idx].ActiveURL = r.url
			peers[r.idx].Reachable = true
			peers[r.idx].ListConn = r.conn
		} else {
			log.Warn("peer unreachable: %s", peers[r.idx].Label())
		}
	}
}

func tryConnect(u *urlutil.NormalizedURL, opts cli.Options) fsutil.PeerFS {
	timeout := time.Duration(opts.CT) * time.Second
	if u.ConnTimeout > 0 {
		timeout = time.Duration(u.ConnTimeout) * time.Second
	}

	switch u.Scheme {
	case "file":
		// Auto-create root dir (use OSPath for native filesystem operations)
		osPath := u.OSPath()
		if err := os.MkdirAll(osPath, 0755); err != nil {
			log.Debug("cannot create local path %s: %v", osPath, err)
			return nil
		}
		return fsutil.NewLocalFS(osPath)
	case "sftp":
		conn, err := fsutil.DialSFTP(u.User, u.Password, u.Host, u.Port, u.Path, timeout)
		if err != nil {
			log.Debug("sftp connect to %s failed: %v", u.String(), err)
			return nil
		}
		// Auto-create root dir
		conn.CreateDir("")
		return conn
	}
	return nil
}

// singlePeerSnapshot walks the peer's filesystem and records a snapshot.
func singlePeerSnapshot(p *peer.Peer, dirPath string) {
	entries, err := p.ListConn.ListDir(dirPath)
	if err != nil {
		log.Error("list %s on %s: %v", dirPath, p.Label(), err)
		return
	}

	nowStr := snapshot.NowStr()

	// Build set of live names for absent-file detection
	liveNames := make(map[string]bool)
	for _, e := range entries {
		if e.Name == ".kitchensync" {
			continue
		}
		liveNames[e.Name] = true
	}

	// Check snapshot children for absent files that need tombstoning
	children, err := p.Snap.GetChildren(dirPath)
	if err == nil {
		for _, row := range children {
			if !liveNames[row.Basename] && !row.DeletedTime.Valid {
				// File/dir was in snapshot but is now absent -- tombstone it
				relPath := joinRelPath(dirPath, row.Basename)
				p.Snap.SetDeletedTime(relPath)
				if row.ByteSize == -1 {
					// Directory: cascade tombstones to children
					p.Snap.CascadeTombstones(relPath, nowStr)
				}
			}
		}
	}

	// Record present files/dirs
	for _, e := range entries {
		if e.Name == ".kitchensync" {
			continue
		}
		entryPath := joinRelPath(dirPath, e.Name)
		parentPath := dirPath

		if e.IsDir {
			p.Snap.Upsert(entryPath, parentPath, e.Name, "0000-00-00_00-00-00_000000Z", -1, &nowStr, nil)
			singlePeerSnapshot(p, entryPath)
		} else {
			modStr := snapshot.FormatModTime(e.ModTime)
			p.Snap.Upsert(entryPath, parentPath, e.Name, modStr, e.ByteSize, &nowStr, nil)
		}
	}
}

// joinRelPath joins a directory and name into a relative path.
func joinRelPath(dir, name string) string {
	if dir == "" || dir == "." {
		return name
	}
	return dir + "/" + name
}

// singlePeerCleanup walks directories and cleans up expired BAK/TMP entries.
func singlePeerCleanup(p *peer.Peer, dirPath string, opts cli.Options) {
	if opts.BD == 0 && opts.XD == 0 {
		return
	}

	// Check for .kitchensync directory at this level
	ksDir := joinRelPath(dirPath, ".kitchensync")
	if exists, _ := p.ListConn.Exists(ksDir); exists {
		if opts.BD > 0 {
			cleanupExpired(p.ListConn, joinRelPath(ksDir, "BAK"), opts.BD)
		}
		if opts.XD > 0 {
			cleanupExpired(p.ListConn, joinRelPath(ksDir, "TMP"), opts.XD)
		}
	}

	// Recurse into subdirectories
	entries, err := p.ListConn.ListDir(dirPath)
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir && e.Name != ".kitchensync" {
			singlePeerCleanup(p, joinRelPath(dirPath, e.Name), opts)
		}
	}
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
		t, err := timestamp.ParseTime(entry.Name)
		if err != nil {
			continue
		}
		if t.Before(cutoff) {
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

func downloadSnapshot(p *peer.Peer) error {
	tmpDir, err := os.MkdirTemp("", "kitchensync-snap-*")
	if err != nil {
		return err
	}
	localDBPath := tmpDir + "/snapshot.db"

	// Try to read peer's .kitchensync/snapshot.db
	reader, err := p.ListConn.ReadFile(".kitchensync/snapshot.db")
	if err != nil {
		// No snapshot on peer -- create empty
		db, err := snapshot.Open(localDBPath)
		if err != nil {
			return err
		}
		p.Snap = db
		p.AutoSubordinate = true
		return nil
	}

	// Copy to local temp
	f, err := os.Create(localDBPath)
	if err != nil {
		reader.Close()
		return err
	}
	_, err = io.Copy(f, reader)
	reader.Close()
	f.Close()
	if err != nil {
		return err
	}

	db, err := snapshot.Open(localDBPath)
	if err != nil {
		return err
	}
	p.Snap = db
	return nil
}

func uploadSnapshot(p *peer.Peer) error {
	if p.Snap == nil {
		return nil
	}
	dbPath := p.Snap.Path()
	p.Snap.Close()

	// Read local snapshot file
	f, err := os.Open(dbPath)
	if err != nil {
		return err
	}
	defer f.Close()

	// Upload via TMP staging + atomic rename
	ts := timestamp.FormatTime(timestamp.Now())
	tmpPath := path.Join(".kitchensync/TMP", ts, "snapshot.db")

	if err := p.ListConn.WriteFile(tmpPath, f); err != nil {
		return fmt.Errorf("write tmp snapshot: %w", err)
	}

	finalPath := ".kitchensync/snapshot.db"
	if err := p.ListConn.Rename(tmpPath, finalPath); err != nil {
		return fmt.Errorf("rename snapshot: %w", err)
	}

	// Clean up TMP dir
	tmpDir := path.Dir(tmpPath)
	p.ListConn.DeleteDir(tmpDir)

	return nil
}
