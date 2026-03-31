package fsutil

import (
	"fmt"
	"io"
	"net"
	"os"
	"path"
	"path/filepath"
	"time"

	"github.com/pkg/sftp"
	"golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/agent"
	"golang.org/x/crypto/ssh/knownhosts"
)

// SFTPFS implements PeerFS for sftp:// paths.
type SFTPFS struct {
	Root   string
	client *sftp.Client
	sshc   *ssh.Client
}

// DialSFTP connects to an SFTP server and returns a PeerFS.
func DialSFTP(user, password, host string, port int, rootPath string, timeout time.Duration) (*SFTPFS, error) {
	if port == 0 {
		port = 22
	}

	authMethods := buildAuthMethods(password)

	knownHostsPath := filepath.Join(os.Getenv("HOME"), ".ssh", "known_hosts")
	if home, _ := os.UserHomeDir(); home != "" {
		knownHostsPath = filepath.Join(home, ".ssh", "known_hosts")
	}
	hostKeyCallback := ssh.InsecureIgnoreHostKey()
	if cb, err := knownhosts.New(knownHostsPath); err == nil {
		hostKeyCallback = cb
	}

	config := &ssh.ClientConfig{
		User:            user,
		Auth:            authMethods,
		HostKeyCallback: hostKeyCallback,
		Timeout:         timeout,
	}

	addr := fmt.Sprintf("%s:%d", host, port)
	conn, err := net.DialTimeout("tcp", addr, timeout)
	if err != nil {
		return nil, fmt.Errorf("dial %s: %w", addr, err)
	}

	sshConn, chans, reqs, err := ssh.NewClientConn(conn, addr, config)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("ssh handshake %s: %w", addr, err)
	}

	sshClient := ssh.NewClient(sshConn, chans, reqs)
	sftpClient, err := sftp.NewClient(sshClient)
	if err != nil {
		sshClient.Close()
		return nil, fmt.Errorf("sftp session %s: %w", addr, err)
	}

	return &SFTPFS{Root: rootPath, client: sftpClient, sshc: sshClient}, nil
}

func buildAuthMethods(password string) []ssh.AuthMethod {
	var methods []ssh.AuthMethod

	// Inline password
	if password != "" {
		methods = append(methods, ssh.Password(password))
	}

	// SSH agent
	if sock := os.Getenv("SSH_AUTH_SOCK"); sock != "" {
		if conn, err := net.Dial("unix", sock); err == nil {
			methods = append(methods, ssh.PublicKeysCallback(agent.NewClient(conn).Signers))
		}
	}

	// Key files
	home, _ := os.UserHomeDir()
	if home == "" {
		home = os.Getenv("HOME")
	}
	for _, name := range []string{"id_ed25519", "id_ecdsa", "id_rsa"} {
		keyPath := filepath.Join(home, ".ssh", name)
		key, err := os.ReadFile(keyPath)
		if err != nil {
			continue
		}
		signer, err := ssh.ParsePrivateKey(key)
		if err != nil {
			continue
		}
		methods = append(methods, ssh.PublicKeys(signer))
	}

	return methods
}

func (f *SFTPFS) abs(p string) string {
	if p == "" || p == "." || p == "/" {
		return f.Root
	}
	return path.Join(f.Root, p)
}

func (f *SFTPFS) ListDir(dirPath string) ([]DirEntry, error) {
	entries, err := f.client.ReadDir(f.abs(dirPath))
	if err != nil {
		return nil, err
	}
	var result []DirEntry
	for _, info := range entries {
		// Skip symlinks and special files
		if info.Mode()&os.ModeSymlink != 0 {
			continue
		}
		if !info.Mode().IsRegular() && !info.IsDir() {
			continue
		}
		de := DirEntry{
			Name:    info.Name(),
			IsDir:   info.IsDir(),
			ModTime: info.ModTime().UTC(),
		}
		if info.IsDir() {
			de.ByteSize = -1
		} else {
			de.ByteSize = info.Size()
		}
		result = append(result, de)
	}
	return result, nil
}

func (f *SFTPFS) Stat(p string) (DirEntry, error) {
	info, err := f.client.Lstat(f.abs(p))
	if err != nil {
		return DirEntry{}, err
	}
	if info.Mode()&os.ModeSymlink != 0 {
		return DirEntry{}, os.ErrNotExist
	}
	if !info.Mode().IsRegular() && !info.IsDir() {
		return DirEntry{}, os.ErrNotExist
	}
	de := DirEntry{
		Name:    info.Name(),
		IsDir:   info.IsDir(),
		ModTime: info.ModTime().UTC(),
	}
	if info.IsDir() {
		de.ByteSize = -1
	} else {
		de.ByteSize = info.Size()
	}
	return de, nil
}

func (f *SFTPFS) ReadFile(p string) (io.ReadCloser, error) {
	return f.client.Open(f.abs(p))
}

func (f *SFTPFS) WriteFile(p string, r io.Reader) error {
	fp := f.abs(p)
	if err := f.client.MkdirAll(path.Dir(fp)); err != nil {
		return err
	}
	file, err := f.client.Create(fp)
	if err != nil {
		return err
	}
	_, err = io.Copy(file, r)
	file.Close()
	return err
}

func (f *SFTPFS) Rename(src, dst string) error {
	s := f.abs(src)
	d := f.abs(dst)
	if err := f.client.MkdirAll(path.Dir(d)); err != nil {
		return err
	}
	// Remove destination if it exists (SFTP rename doesn't overwrite)
	f.client.Remove(d)
	return f.client.Rename(s, d)
}

func (f *SFTPFS) DeleteFile(p string) error {
	return f.client.Remove(f.abs(p))
}

func (f *SFTPFS) CreateDir(p string) error {
	return f.client.MkdirAll(f.abs(p))
}

func (f *SFTPFS) DeleteDir(p string) error {
	return f.client.Remove(f.abs(p))
}

func (f *SFTPFS) SetModTime(p string, t time.Time) error {
	return f.client.Chtimes(f.abs(p), t, t)
}

func (f *SFTPFS) GetPermissions(p string) (uint32, error) {
	info, err := f.client.Stat(f.abs(p))
	if err != nil {
		return 0, err
	}
	return uint32(info.Mode().Perm()), nil
}

func (f *SFTPFS) SetPermissions(p string, perm uint32) error {
	return f.client.Chmod(f.abs(p), os.FileMode(perm))
}

func (f *SFTPFS) Exists(p string) (bool, error) {
	_, err := f.client.Lstat(f.abs(p))
	if os.IsNotExist(err) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

func (f *SFTPFS) Close() error {
	f.client.Close()
	return f.sshc.Close()
}
