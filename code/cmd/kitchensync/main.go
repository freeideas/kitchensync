package main

import (
	"fmt"
	"io"
	"kitchensync/internal/args"
	"kitchensync/internal/db"
	"kitchensync/internal/engine"
	"kitchensync/internal/fsys"
	"kitchensync/internal/lock"
	"kitchensync/internal/logx"
	"kitchensync/internal/pool"
	"kitchensync/internal/ts"
	"kitchensync/internal/urlnorm"
	"kitchensync/internal/watch"
	"os"
	"os/signal"
	"path/filepath"
	"sort"
	"syscall"
	"time"
)

func main() {
	cfg, err := args.Parse(os.Args[1:])
	if err != nil {
		if _, ok := err.(*args.HelpRequested); ok {
			fmt.Println(err.Error())
			os.Exit(0)
		}
		fmt.Println(err.Error())
		os.Exit(1)
	}

	if err := logx.SetLevel(cfg.Options.VL); err != nil {
		fmt.Println(err.Error())
		os.Exit(1)
	}

	poolMgr := pool.NewManager()
	defer poolMgr.CloseAll()

	// Build sync peers
	var syncPeers []*engine.SyncPeer
	var canonPeer *engine.SyncPeer
	var peerURLs []string

	for _, pCfg := range cfg.Peers {
		sp := &engine.SyncPeer{
			Config:        pCfg,
			IsSubordinate: pCfg.IsSubordinate,
		}
		syncPeers = append(syncPeers, sp)
	}

	// Connect to all peers in parallel
	for _, sp := range syncPeers {
		connected := false
		for _, pURL := range sp.Config.URLs {
			mc := cfg.Options.MC
			ct := cfg.Options.CT
			if pURL.MC > 0 {
				mc = pURL.MC
			}
			if pURL.CT > 0 {
				ct = pURL.CT
			}

			password := pool.ExtractPassword(pURL.Raw)
			scheme := urlnorm.Scheme(pURL.Normalized)

			var listFS fsys.PeerFS
			if scheme == "file" {
				osPath := urlnorm.OSPath(pURL.Normalized)
				// Auto-create root dir
				if err := os.MkdirAll(osPath, 0755); err != nil {
					logx.Warn("cannot create %s: %v", osPath, err)
					continue
				}
				// Also create .kitchensync dir
				os.MkdirAll(filepath.Join(osPath, ".kitchensync"), 0755)
				listFS = fsys.NewLocalFS(osPath)
			} else if scheme == "sftp" {
				user, host, port, rootPath := urlnorm.ParseSFTP(pURL.Normalized)
				if user == "" {
					user = currentUser()
				}
				sftpFS, err := fsys.ConnectSFTP(user, host, port, rootPath, password, time.Duration(ct)*time.Second)
				if err != nil {
					logx.Warn("connect failed %s: %v", pURL.Normalized, err)
					continue
				}
				// Auto-create root
				sftpFS.CreateDir(".")
				sftpFS.CreateDir(".kitchensync")
				listFS = sftpFS
			}

			if listFS != nil {
				sp.ActiveURL = pURL.Normalized
				sp.ListingFS = listFS
				sp.Reachable = true
				sp.Password = password
				sp.Pool = poolMgr.GetOrCreate(pURL.Normalized, mc, ct, password)
				connected = true
				logx.Debug("connected to %s", pURL.Normalized)
				break
			}
		}
		if !connected {
			logx.Warn("peer unreachable: %s", sp.Config.URLs[0].Normalized)
		}
		if sp.IsCanon() {
			canonPeer = sp
		}
		if sp.Reachable {
			peerURLs = append(peerURLs, sp.ActiveURL)
		}
	}
	sort.Strings(peerURLs)

	// Validate reachability
	reachableCount := 0
	for _, sp := range syncPeers {
		if sp.Reachable {
			reachableCount++
		}
	}

	if reachableCount == 0 {
		fmt.Println("no peers reachable")
		os.Exit(1)
	}
	if canonPeer != nil && !canonPeer.Reachable {
		fmt.Println("canon peer unreachable")
		os.Exit(1)
	}
	if reachableCount == 1 && len(syncPeers) >= 2 {
		logx.Warn("only one peer reachable -- running in snapshot-only mode")
	}

	// Instance lock check
	for _, sp := range syncPeers {
		if !sp.Reachable {
			continue
		}
		lockData := readLockData(sp)
		overlap, err := lock.CheckExisting(lockData, peerURLs)
		if overlap {
			fmt.Printf("overlapping instance: %v\n", err)
			os.Exit(1)
		}
	}

	instanceLock := lock.New(peerURLs)
	if err := instanceLock.Bind(); err != nil {
		fmt.Printf("instance lock failed: %v\n", err)
		os.Exit(1)
	}
	defer instanceLock.Close()

	// Write lock files
	for _, sp := range syncPeers {
		if !sp.Reachable {
			continue
		}
		writeLockData(sp, instanceLock.Port())
	}
	defer func() {
		for _, sp := range syncPeers {
			if !sp.Reachable {
				continue
			}
			deleteLockData(sp)
		}
	}()

	// Download snapshots
	tmpDir, err := os.MkdirTemp("", "kitchensync-snap-*")
	if err != nil {
		fmt.Printf("failed to create temp dir: %v\n", err)
		os.Exit(1)
	}
	defer os.RemoveAll(tmpDir)

	hasAnyRows := false
	for _, sp := range syncPeers {
		if !sp.Reachable {
			continue
		}
		localDBPath := filepath.Join(tmpDir, urlSafeFilename(sp.ActiveURL)+".db")
		downloaded := downloadSnapshot(sp, localDBPath)

		snapDB, err := db.Open(localDBPath)
		if err != nil {
			logx.Warn("snapshot open failed for %s: %v", sp.ActiveURL, err)
			snapDB, _ = db.Open(localDBPath)
			snapDB.Init()
			if !sp.IsCanon() {
				sp.AutoSub = true
			}
			sp.Snapshot = snapDB
			continue
		}

		if !downloaded {
			snapDB.Init()
			if !sp.IsCanon() {
				sp.AutoSub = true
			}
		} else {
			// Check if we need to init (in case downloaded DB is empty/corrupt)
			snapDB.Init()
		}

		rows, _ := snapDB.HasRows()
		if rows {
			hasAnyRows = true
		} else if !sp.IsCanon() {
			sp.AutoSub = true
		}

		sp.Snapshot = snapDB
	}

	// Check first-sync requirement
	if len(syncPeers) >= 2 && !hasAnyRows && canonPeer == nil {
		fmt.Println("First sync? Mark the authoritative peer with a leading +")
		os.Exit(1)
	}

	// Check contributing peer availability
	if len(syncPeers) >= 2 {
		hasContributor := false
		for _, sp := range syncPeers {
			if sp.Reachable && !sp.IsSub() {
				hasContributor = true
				break
			}
		}
		if !hasContributor {
			fmt.Println("No contributing peer reachable -- cannot make sync decisions")
			os.Exit(1)
		}
	}

	// Purge old tombstones
	if cfg.Options.TD > 0 {
		for _, sp := range syncPeers {
			if sp.Reachable && sp.Snapshot != nil {
				sp.Snapshot.PurgeTombstones(cfg.Options.TD)
			}
		}
	}

	// Run the sync
	eng := engine.NewEngine(cfg.Options, syncPeers)
	eng.Run()

	// Upload final snapshots
	if !cfg.Options.DryRun {
		eng.UploadSnapshots()
	}

	// Watch mode
	if cfg.Options.Watch {
		if len(syncPeers) == 1 {
			logx.Warn("--watch with one peer: snapshot only")
		}

		w := watch.New(eng, syncPeers)
		if err := w.Start(); err != nil {
			logx.Error("watch start failed: %v", err)
		} else {
			// Wait for shutdown signal
			sigCh := make(chan os.Signal, 1)
			signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

			select {
			case <-sigCh:
				logx.Info("shutting down...")
			case <-instanceLock.ShutdownChan():
				logx.Info("shutdown requested via API...")
			}

			w.Stop()

			// Final snapshot upload
			if !cfg.Options.DryRun {
				eng.UploadSnapshots()
			}
		}
	}

	logx.Info("done")
}

func readLockData(sp *engine.SyncPeer) string {
	reader, err := sp.ListingFS.ReadFile(".kitchensync/lock")
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

func writeLockData(sp *engine.SyncPeer, port int) {
	data := fmt.Sprintf("%d", port)
	sp.ListingFS.WriteFile(".kitchensync/lock", stringReader(data))
}

func deleteLockData(sp *engine.SyncPeer) {
	sp.ListingFS.DeleteFile(".kitchensync/lock")
}

func downloadSnapshot(sp *engine.SyncPeer, localPath string) bool {
	reader, err := sp.ListingFS.ReadFile(".kitchensync/snapshot.db")
	if err != nil {
		return false
	}
	defer reader.Close()

	f, err := os.Create(localPath)
	if err != nil {
		return false
	}
	defer f.Close()

	_, err = io.Copy(f, reader)
	return err == nil
}

func urlSafeFilename(url string) string {
	result := make([]byte, 0, len(url))
	for i := 0; i < len(url); i++ {
		c := url[i]
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '_' {
			result = append(result, c)
		} else {
			result = append(result, '_')
		}
	}
	return string(result)
}

func currentUser() string {
	if u := os.Getenv("USER"); u != "" {
		return u
	}
	if u := os.Getenv("USERNAME"); u != "" {
		return u
	}
	return "root"
}

type stringReaderImpl struct {
	data []byte
	pos  int
}

func stringReader(s string) io.Reader {
	return &stringReaderImpl{data: []byte(s)}
}

func (r *stringReaderImpl) Read(p []byte) (int, error) {
	if r.pos >= len(r.data) {
		return 0, io.EOF
	}
	n := copy(p, r.data[r.pos:])
	r.pos += n
	return n, nil
}

func init() {
	_ = ts.Now
}
