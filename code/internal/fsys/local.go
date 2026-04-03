package fsys

import (
	"io"
	"os"
	"path/filepath"
	"time"
)

type LocalFS struct {
	root string
}

func NewLocalFS(root string) *LocalFS {
	return &LocalFS{root: root}
}

func (fs *LocalFS) abs(path string) string {
	if path == "" || path == "." {
		return fs.root
	}
	return filepath.Join(fs.root, filepath.FromSlash(path))
}

func (fs *LocalFS) ListDir(path string) ([]Entry, error) {
	dirPath := fs.abs(path)
	entries, err := os.ReadDir(dirPath)
	if err != nil {
		return nil, err
	}

	var result []Entry
	for _, e := range entries {
		info, err := e.Info()
		if err != nil {
			continue
		}
		// Skip symlinks and special files
		if info.Mode()&os.ModeSymlink != 0 || info.Mode().Type()&os.ModeType != 0 && !info.IsDir() {
			continue
		}
		// For entries from ReadDir, check if it's a symlink via Lstat
		linfo, err := os.Lstat(filepath.Join(dirPath, e.Name()))
		if err != nil {
			continue
		}
		if linfo.Mode()&os.ModeSymlink != 0 {
			continue
		}
		if !linfo.Mode().IsRegular() && !linfo.IsDir() {
			continue
		}

		entry := Entry{
			Name:    e.Name(),
			IsDir:   linfo.IsDir(),
			ModTime: linfo.ModTime().UTC(),
		}
		if linfo.IsDir() {
			entry.ByteSize = -1
		} else {
			entry.ByteSize = linfo.Size()
		}
		result = append(result, entry)
	}
	return result, nil
}

func (fs *LocalFS) Stat(path string) (*Entry, error) {
	p := fs.abs(path)
	// Use Lstat to detect symlinks
	linfo, err := os.Lstat(p)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	if linfo.Mode()&os.ModeSymlink != 0 {
		return nil, nil // symlinks reported as not found
	}
	if !linfo.Mode().IsRegular() && !linfo.IsDir() {
		return nil, nil // special files reported as not found
	}
	entry := &Entry{
		Name:    filepath.Base(p),
		IsDir:   linfo.IsDir(),
		ModTime: linfo.ModTime().UTC(),
	}
	if linfo.IsDir() {
		entry.ByteSize = -1
	} else {
		entry.ByteSize = linfo.Size()
	}
	return entry, nil
}

func (fs *LocalFS) ReadFile(path string) (io.ReadCloser, error) {
	return os.Open(fs.abs(path))
}

func (fs *LocalFS) WriteFile(path string, r io.Reader) error {
	p := fs.abs(path)
	if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
		return err
	}
	f, err := os.Create(p)
	if err != nil {
		return err
	}
	defer f.Close()
	_, err = io.Copy(f, r)
	return err
}

func (fs *LocalFS) Rename(src, dst string) error {
	srcPath := fs.abs(src)
	dstPath := fs.abs(dst)
	if err := os.MkdirAll(filepath.Dir(dstPath), 0755); err != nil {
		return err
	}
	return os.Rename(srcPath, dstPath)
}

func (fs *LocalFS) DeleteFile(path string) error {
	return os.Remove(fs.abs(path))
}

func (fs *LocalFS) CreateDir(path string) error {
	return os.MkdirAll(fs.abs(path), 0755)
}

func (fs *LocalFS) DeleteDir(path string) error {
	return os.Remove(fs.abs(path))
}

func (fs *LocalFS) SetModTime(path string, t time.Time) error {
	return os.Chtimes(fs.abs(path), t, t)
}

func (fs *LocalFS) GetPermissions(path string) (os.FileMode, error) {
	info, err := os.Stat(fs.abs(path))
	if err != nil {
		return 0, err
	}
	return info.Mode().Perm(), nil
}

func (fs *LocalFS) SetPermissions(path string, mode os.FileMode) error {
	return os.Chmod(fs.abs(path), mode)
}

func (fs *LocalFS) Close() error {
	return nil
}
