package engine

import (
	"io"
	"kitchensync/internal/fsys"
	"kitchensync/internal/logx"
	"kitchensync/internal/pool"
	"kitchensync/internal/ts"
	"os"
	"path"
	"runtime"
	"sync"
	"time"

	"github.com/google/uuid"
)

const (
	chunkSize  = 64 * 1024 // 64KB
	chanBuffer = 16
)

type CopyJob struct {
	SrcPeer *SyncPeer
	DstPeer *SyncPeer
	RelPath string
	WinMod  time.Time
	WinSize int64
}

type CopyQueue struct {
	jobs    chan CopyJob
	wg      sync.WaitGroup
	onCopy  func() // called after each successful copy (for checkpoint timer)
}

func NewCopyQueue(workers int, onCopy func()) *CopyQueue {
	q := &CopyQueue{
		jobs:   make(chan CopyJob, 1000),
		onCopy: onCopy,
	}
	for i := 0; i < workers; i++ {
		q.wg.Add(1)
		go q.worker()
	}
	return q
}

func (q *CopyQueue) Enqueue(job CopyJob) {
	q.jobs <- job
}

func (q *CopyQueue) CloseAndWait(timeout time.Duration) {
	close(q.jobs)
	done := make(chan struct{})
	go func() {
		q.wg.Wait()
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(timeout):
		logx.Warn("copy queue timeout after %v", timeout)
	}
}

func (q *CopyQueue) worker() {
	defer q.wg.Done()
	for job := range q.jobs {
		executeCopy(job)
		if q.onCopy != nil {
			q.onCopy()
		}
	}
}

func executeCopy(job CopyJob) {
	srcConn, dstConn, err := pool.AcquireOrdered(job.SrcPeer.Pool, job.DstPeer.Pool)
	if err != nil {
		logx.Error("copy acquire failed %s: %v", job.RelPath, err)
		return
	}
	defer job.SrcPeer.Pool.Release(srcConn)
	defer job.DstPeer.Pool.Release(dstConn)

	timestamp := ts.Now()
	uid := uuid.New().String()
	parentDir := path.Dir(job.RelPath)
	if parentDir == "." {
		parentDir = ""
	}
	basename := path.Base(job.RelPath)

	var tmpPath string
	if parentDir == "" {
		tmpPath = path.Join(".kitchensync", "TMP", timestamp, uid, basename)
	} else {
		tmpPath = path.Join(parentDir, ".kitchensync", "TMP", timestamp, uid, basename)
	}

	logx.Trace("pipe reader-start src=%s file=%s", job.SrcPeer.ActiveURL, job.RelPath)
	logx.Trace("pipe writer-start dst=%s file=%s", job.DstPeer.ActiveURL, job.RelPath)

	// Pipelined transfer: reader -> channel -> writer
	ch := make(chan dataChunk, chanBuffer)

	var transferErr error
	var transferMu sync.Mutex
	var wg sync.WaitGroup

	// Reader goroutine
	wg.Add(1)
	go func() {
		defer wg.Done()
		defer close(ch)
		reader, err := srcConn.ReadFile(job.RelPath)
		if err != nil {
			transferMu.Lock()
			transferErr = err
			transferMu.Unlock()
			return
		}
		defer reader.Close()
		for {
			buf := make([]byte, chunkSize)
			n, readErr := reader.Read(buf)
			if n > 0 {
				ch <- dataChunk{data: buf[:n]}
			}
			if readErr != nil {
				if readErr != io.EOF {
					transferMu.Lock()
					transferErr = readErr
					transferMu.Unlock()
				}
				return
			}
		}
	}()

	// Writer goroutine
	wg.Add(1)
	go func() {
		defer wg.Done()
		cr := &chanToReader{ch: ch}
		err := dstConn.WriteFile(tmpPath, cr)
		if err != nil {
			transferMu.Lock()
			transferErr = err
			transferMu.Unlock()
		}
		logx.Trace("pipe writer-done  dst=%s file=%s", job.DstPeer.ActiveURL, job.RelPath)
	}()

	wg.Wait()
	logx.Trace("pipe reader-done  src=%s file=%s", job.SrcPeer.ActiveURL, job.RelPath)

	if transferErr != nil {
		logx.Error("copy failed %s: %v", job.RelPath, transferErr)
		dstConn.DeleteFile(tmpPath)
		return
	}

	// Displace existing file at target
	existing, _ := dstConn.Stat(job.RelPath)
	if existing != nil && !existing.IsDir {
		bakTimestamp := ts.Now()
		var bakPath string
		if parentDir == "" {
			bakPath = path.Join(".kitchensync", "BAK", bakTimestamp, basename)
		} else {
			bakPath = path.Join(parentDir, ".kitchensync", "BAK", bakTimestamp, basename)
		}
		if err := dstConn.Rename(job.RelPath, bakPath); err != nil {
			logx.Error("displace failed %s on %s: %v", job.RelPath, job.DstPeer.ActiveURL, err)
			dstConn.DeleteFile(tmpPath)
			return
		}
	}

	// Atomic swap
	if err := dstConn.Rename(tmpPath, job.RelPath); err != nil {
		logx.Error("rename tmp->final failed %s: %v", job.RelPath, err)
		dstConn.DeleteFile(tmpPath)
		return
	}

	// Set mod_time
	if err := dstConn.SetModTime(job.RelPath, job.WinMod); err != nil {
		logx.Warn("set mod_time failed %s on %s: %v", job.RelPath, job.DstPeer.ActiveURL, err)
	}

	// Best-effort permission copy
	if runtime.GOOS != "windows" {
		perm, err := srcConn.GetPermissions(job.RelPath)
		if err == nil {
			if err := dstConn.SetPermissions(job.RelPath, perm); err != nil {
				logx.Debug("set permissions failed %s: %v", job.RelPath, err)
			}
		}
	}

	// Cleanup empty TMP parents
	cleanupEmptyParents(dstConn, tmpPath)

	// Post-copy snapshot update
	now := ts.Now()
	job.DstPeer.Snapshot.SetLastSeen(job.RelPath, now)
}

// chanToReader adapts a chunk channel to an io.Reader
type chanToReader struct {
	ch  <-chan dataChunk
	buf []byte
}

type dataChunk struct {
	data []byte
	err  error
}

func (r *chanToReader) Read(p []byte) (int, error) {
	if len(r.buf) > 0 {
		n := copy(p, r.buf)
		r.buf = r.buf[n:]
		return n, nil
	}
	c, ok := <-r.ch
	if !ok {
		return 0, io.EOF
	}
	if c.err != nil {
		return 0, c.err
	}
	n := copy(p, c.data)
	if n < len(c.data) {
		r.buf = c.data[n:]
	}
	return n, nil
}

func cleanupEmptyParents(conn fsys.PeerFS, tmpPath string) {
	dir := path.Dir(tmpPath)
	for {
		if dir == "." || dir == "" || dir == "/" {
			break
		}
		base := path.Base(dir)
		if base == ".kitchensync" {
			break
		}
		entries, err := conn.ListDir(dir)
		if err != nil || len(entries) > 0 {
			break
		}
		conn.DeleteDir(dir)
		dir = path.Dir(dir)
	}
}

func displaceEntry(peer *SyncPeer, relPath string, dryRun bool) error {
	if dryRun {
		return nil
	}
	timestamp := ts.Now()
	parentDir := path.Dir(relPath)
	if parentDir == "." {
		parentDir = ""
	}
	basename := path.Base(relPath)

	var bakPath string
	if parentDir == "" {
		bakPath = path.Join(".kitchensync", "BAK", timestamp, basename)
	} else {
		bakPath = path.Join(parentDir, ".kitchensync", "BAK", timestamp, basename)
	}
	return peer.ListingFS.Rename(relPath, bakPath)
}

func init() {
	_ = os.MkdirAll
}
