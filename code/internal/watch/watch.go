package watch

import (
	"errors"
	"io"
	"kitchensync/internal/db"
	"kitchensync/internal/engine"
	"kitchensync/internal/fsys"
	"kitchensync/internal/logx"
	"kitchensync/internal/ts"
	"kitchensync/internal/urlnorm"
	"path/filepath"
	"strings"
	"sync"

	"github.com/fsnotify/fsnotify"
)

type Watcher struct {
	eng      *engine.Engine
	peers    []*engine.SyncPeer
	watcher  *fsnotify.Watcher
	inflight sync.Map
	stopCh   chan struct{}
}

func New(eng *engine.Engine, peers []*engine.SyncPeer) *Watcher {
	return &Watcher{
		eng:    eng,
		peers:  peers,
		stopCh: make(chan struct{}),
	}
}

func (w *Watcher) Start() error {
	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		return err
	}
	w.watcher = watcher

	watchCount := 0
	for _, p := range w.peers {
		if urlnorm.Scheme(p.ActiveURL) != "file" {
			continue
		}
		osPath := urlnorm.OSPath(p.ActiveURL)
		if err := watcher.Add(osPath); err != nil {
			logx.Warn("watch failed: %s (%v)", osPath, err)
			continue
		}
		logx.Info("watching %s", p.ActiveURL)
		watchCount++
	}

	if watchCount == 0 {
		watcher.Close()
		return errors.New("no local peers could be watched")
	}

	go w.eventLoop()
	return nil
}

func (w *Watcher) Stop() {
	close(w.stopCh)
	if w.watcher != nil {
		w.watcher.Close()
	}
}

func (w *Watcher) AddInflight(path string) {
	w.inflight.Store(path, true)
}

func (w *Watcher) RemoveInflight(path string) {
	w.inflight.Delete(path)
}

func (w *Watcher) eventLoop() {
	for {
		select {
		case <-w.stopCh:
			return
		case event, ok := <-w.watcher.Events:
			if !ok {
				return
			}
			w.handleEvent(event)
		case err, ok := <-w.watcher.Errors:
			if !ok {
				return
			}
			logx.Warn("watch error: %v", err)
		}
	}
}

func (w *Watcher) handleEvent(event fsnotify.Event) {
	absPath := filepath.ToSlash(event.Name)

	if strings.Contains(absPath, ".kitchensync") {
		return
	}
	if _, ok := w.inflight.Load(absPath); ok {
		return
	}

	var watchedPeer *engine.SyncPeer
	var relPath string
	for _, p := range w.peers {
		if urlnorm.Scheme(p.ActiveURL) != "file" {
			continue
		}
		root := urlnorm.OSPath(p.ActiveURL)
		root = filepath.ToSlash(root)
		if strings.HasPrefix(absPath, root+"/") {
			relPath = absPath[len(root)+1:]
			watchedPeer = p
			break
		}
	}

	if watchedPeer == nil {
		return
	}

	// Debounce via snapshot comparison
	currentStat, _ := watchedPeer.ListingFS.Stat(relPath)
	snapRow, _ := watchedPeer.Snapshot.Lookup(relPath)
	if currentStat != nil && snapRow != nil && !snapRow.DeletedTime.Valid {
		snapTime, err := ts.Parse(snapRow.ModTime)
		if err == nil {
			diff := currentStat.ModTime.Sub(snapTime)
			if diff < 0 {
				diff = -diff
			}
			if diff <= engine.TimeTolerance {
				return
			}
		}
	}

	if currentStat != nil {
		logx.Info("W C %s", relPath)
	} else {
		logx.Info("W X %s", relPath)
	}
}

func init() {
	_ = io.ReadAll
	_ = db.SentinelPath
	_ = (*fsys.Entry)(nil)
}
