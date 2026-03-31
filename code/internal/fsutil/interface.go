package fsutil

import (
	"io"
	"time"
)

// DirEntry represents a single directory listing entry.
type DirEntry struct {
	Name     string
	IsDir    bool
	ModTime  time.Time
	ByteSize int64 // file size for files, -1 for directories
}

// PeerFS is the filesystem interface that both local and SFTP implement.
type PeerFS interface {
	// ListDir lists immediate children. Returns only regular files and directories.
	// Symlinks and special files are silently omitted.
	ListDir(path string) ([]DirEntry, error)

	// Stat returns info about a path. Returns ErrNotFound for symlinks/specials.
	Stat(path string) (DirEntry, error)

	// ReadFile opens a file for streaming read.
	ReadFile(path string) (io.ReadCloser, error)

	// WriteFile creates/overwrites a file from stream, creating parent dirs as needed.
	WriteFile(path string, r io.Reader) error

	// Rename performs a same-filesystem rename.
	Rename(src, dst string) error

	// DeleteFile removes a file.
	DeleteFile(path string) error

	// CreateDir creates a directory and parents as needed.
	CreateDir(path string) error

	// DeleteDir removes an empty directory.
	DeleteDir(path string) error

	// SetModTime sets the modification time of a path.
	SetModTime(path string, t time.Time) error

	// GetPermissions returns the file permissions.
	GetPermissions(path string) (uint32, error)

	// SetPermissions sets file permissions. Best-effort.
	SetPermissions(path string, perm uint32) error

	// Exists checks if a path exists.
	Exists(path string) (bool, error)

	// Close releases resources.
	Close() error
}
