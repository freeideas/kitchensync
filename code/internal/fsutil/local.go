package fsutil

import (
	"io"
	"os"
	"path/filepath"
	"time"
)

// LocalFS implements PeerFS for local file:// paths.
type LocalFS struct {
	Root string
}

func NewLocalFS(root string) *LocalFS {
	return &LocalFS{Root: root}
}

func (f *LocalFS) abs(path string) string {
	if path == "" || path == "." || path == "/" {
		return f.Root
	}
	return filepath.Join(f.Root, filepath.FromSlash(path))
}

func (f *LocalFS) ListDir(path string) ([]DirEntry, error) {
	entries, err := os.ReadDir(f.abs(path))
	if err != nil {
		return nil, err
	}
	var result []DirEntry
	for _, e := range entries {
		// Skip symlinks and special files
		if e.Type()&os.ModeSymlink != 0 {
			continue
		}
		if !e.Type().IsRegular() && !e.IsDir() {
			continue
		}
		info, err := e.Info()
		if err != nil {
			continue
		}
		// Double-check it's not a symlink (Info() follows symlinks)
		linfo, err := os.Lstat(filepath.Join(f.abs(path), e.Name()))
		if err != nil {
			continue
		}
		if linfo.Mode()&os.ModeSymlink != 0 {
			continue
		}
		de := DirEntry{
			Name:    e.Name(),
			IsDir:   e.IsDir(),
			ModTime: info.ModTime().UTC(),
		}
		if e.IsDir() {
			de.ByteSize = -1
		} else {
			de.ByteSize = info.Size()
		}
		result = append(result, de)
	}
	return result, nil
}

func (f *LocalFS) Stat(path string) (DirEntry, error) {
	p := f.abs(path)
	// Check for symlink first
	linfo, err := os.Lstat(p)
	if err != nil {
		return DirEntry{}, err
	}
	if linfo.Mode()&os.ModeSymlink != 0 {
		return DirEntry{}, os.ErrNotExist
	}
	if !linfo.Mode().IsRegular() && !linfo.IsDir() {
		return DirEntry{}, os.ErrNotExist
	}
	de := DirEntry{
		Name:    filepath.Base(p),
		IsDir:   linfo.IsDir(),
		ModTime: linfo.ModTime().UTC(),
	}
	if linfo.IsDir() {
		de.ByteSize = -1
	} else {
		de.ByteSize = linfo.Size()
	}
	return de, nil
}

func (f *LocalFS) ReadFile(path string) (io.ReadCloser, error) {
	return os.Open(f.abs(path))
}

func (f *LocalFS) WriteFile(path string, r io.Reader) error {
	p := f.abs(path)
	if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
		return err
	}
	tmp := p + ".tmp"
	file, err := os.Create(tmp)
	if err != nil {
		return err
	}
	_, err = io.Copy(file, r)
	file.Close()
	if err != nil {
		os.Remove(tmp)
		return err
	}
	return os.Rename(tmp, p)
}

func (f *LocalFS) Rename(src, dst string) error {
	s := f.abs(src)
	d := f.abs(dst)
	if err := os.MkdirAll(filepath.Dir(d), 0755); err != nil {
		return err
	}
	return os.Rename(s, d)
}

func (f *LocalFS) DeleteFile(path string) error {
	return os.Remove(f.abs(path))
}

func (f *LocalFS) CreateDir(path string) error {
	return os.MkdirAll(f.abs(path), 0755)
}

func (f *LocalFS) DeleteDir(path string) error {
	return os.Remove(f.abs(path))
}

func (f *LocalFS) SetModTime(path string, t time.Time) error {
	return os.Chtimes(f.abs(path), t, t)
}

func (f *LocalFS) GetPermissions(path string) (uint32, error) {
	info, err := os.Stat(f.abs(path))
	if err != nil {
		return 0, err
	}
	return uint32(info.Mode().Perm()), nil
}

func (f *LocalFS) SetPermissions(path string, perm uint32) error {
	return os.Chmod(f.abs(path), os.FileMode(perm))
}

func (f *LocalFS) Exists(path string) (bool, error) {
	_, err := os.Lstat(f.abs(path))
	if os.IsNotExist(err) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

func (f *LocalFS) Close() error { return nil }
