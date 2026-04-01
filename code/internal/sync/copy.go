package sync

import (
	"fmt"
	"io"
	"path"

	"github.com/google/uuid"

	"kitchensync/internal/log"
	"kitchensync/internal/peer"
	"kitchensync/internal/pool"
	"kitchensync/internal/snapshot"
	"kitchensync/internal/timestamp"
)

// CopyFile copies a file from src to dst peer using their connection pools.
func CopyFile(srcPeer, dstPeer *peer.Peer, relPath string, modTime string, byteSize int64) error {
	srcConn, dstConn, err := pool.AcquireOrdered(srcPeer.Pool, dstPeer.Pool)
	if err != nil {
		return fmt.Errorf("acquire connections: %w", err)
	}
	defer srcPeer.Pool.Release(srcConn)
	defer dstPeer.Pool.Release(dstConn)

	ts := timestamp.FormatTime(timestamp.Now())
	basename := path.Base(relPath)
	parentDir := path.Dir(relPath)
	if parentDir == "." {
		parentDir = ""
	}
	tmpBase := path.Join(parentDir, ".kitchensync/TMP")
	tmpDir := path.Join(tmpBase, ts, uuid.New().String())
	tmpPath := path.Join(tmpDir, basename)

	log.Trace("pipe reader-start src=%s file=%s", srcPeer.Label(), relPath)
	log.Trace("pipe writer-start dst=%s file=%s", dstPeer.Label(), relPath)

	// Pipelined transfer using a pipe
	pr, pw := io.Pipe()
	errCh := make(chan error, 2)

	// Reader goroutine
	go func() {
		reader, err := srcConn.ReadFile(relPath)
		if err != nil {
			pw.CloseWithError(err)
			errCh <- err
			return
		}
		_, err = io.Copy(pw, reader)
		reader.Close()
		pw.CloseWithError(err)
		errCh <- err
		log.Trace("pipe reader-done  src=%s file=%s", srcPeer.Label(), relPath)
	}()

	// Writer goroutine
	go func() {
		err := dstConn.WriteFile(tmpPath, pr)
		if err != nil {
			// Close the pipe reader so the reader goroutine unblocks if WriteFile
			// returned before consuming all data (e.g., MkdirAll or Create failed).
			pr.CloseWithError(err)
		}
		errCh <- err
		log.Trace("pipe writer-done  dst=%s file=%s", dstPeer.Label(), relPath)
	}()

	// Wait for both
	err1 := <-errCh
	err2 := <-errCh
	if err1 != nil || err2 != nil {
		// Clean up TMP
		dstConn.DeleteFile(tmpPath)
		cleanupEmptyParents(dstConn, tmpDir, tmpBase)
		if err1 != nil {
			return err1
		}
		return err2
	}

	// Displace existing file at target to BAK/
	if exists, _ := dstConn.Exists(relPath); exists {
		bakTs := timestamp.FormatTime(timestamp.Now())
		bakPath := path.Join(parentDir, ".kitchensync/BAK", bakTs, basename)
		if err := dstConn.Rename(relPath, bakPath); err != nil {
			log.Error("displace existing at %s on %s: %v", relPath, dstPeer.Label(), err)
			// Clean up TMP and abort
			dstConn.DeleteFile(tmpPath)
			cleanupEmptyParents(dstConn, tmpDir, tmpBase)
			return err
		}
	}

	// Atomic swap: rename TMP -> final
	if err := dstConn.Rename(tmpPath, relPath); err != nil {
		log.Error("rename tmp to final at %s on %s: %v", relPath, dstPeer.Label(), err)
		dstConn.DeleteFile(tmpPath)
		cleanupEmptyParents(dstConn, tmpDir, tmpBase)
		return err
	}

	// Set mod_time to winning mod_time
	mt, _ := snapshot.ParseModTime(modTime)
	if err := dstConn.SetModTime(relPath, mt); err != nil {
		log.Warn("set mod_time on %s at %s: %v", relPath, dstPeer.Label(), err)
	}

	// Best-effort permission copy
	if perm, err := srcConn.GetPermissions(relPath); err == nil {
		dstConn.SetPermissions(relPath, perm)
	}

	// Clean up empty TMP dirs
	cleanupEmptyParents(dstConn, tmpDir, tmpBase)

	// Post-copy snapshot update: set last_seen = now
	nowStr := snapshot.NowStr()
	dstPeer.Snap.SetLastSeen(relPath, nowStr)

	return nil
}

func cleanupEmptyParents(fs interface{ DeleteDir(string) error }, dir, stopAt string) {
	for dir != stopAt && dir != "" && dir != "." {
		fs.DeleteDir(dir)
		dir = path.Dir(dir)
	}
}
