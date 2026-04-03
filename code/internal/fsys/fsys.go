package fsys

import (
	"io"
	"os"
	"time"
)

type Entry struct {
	Name     string
	IsDir    bool
	ModTime  time.Time
	ByteSize int64 // -1 for directories
}

type PeerFS interface {
	ListDir(path string) ([]Entry, error)
	Stat(path string) (*Entry, error)
	ReadFile(path string) (io.ReadCloser, error)
	WriteFile(path string, r io.Reader) error
	Rename(src, dst string) error
	DeleteFile(path string) error
	CreateDir(path string) error
	DeleteDir(path string) error
	SetModTime(path string, t time.Time) error
	GetPermissions(path string) (os.FileMode, error)
	SetPermissions(path string, mode os.FileMode) error
	Close() error
}
