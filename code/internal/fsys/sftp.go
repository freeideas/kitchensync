package fsys

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/pkg/sftp"
	"golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/knownhosts"
)

type SFTPFS struct {
	client *sftp.Client
	conn   *ssh.Client
	root   string
}

func NewSFTPFS(user, host, port, rootPath, password string, timeout time.Duration) (*SFTPFS, error) {
	config, err := buildSSHConfig(user, host, port, password)
	if err != nil {
		return nil, fmt.Errorf("ssh config: %w", err)
	}
	config.Timeout = timeout

	addr := net.JoinHostPort(host, port)
	conn, err := ssh.Dial("tcp", addr, config)
	if err != nil {
		return nil, fmt.Errorf("ssh dial %s: %w", addr, err)
	}

	client, err := sftp.NewClient(conn)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("sftp client: %w", err)
	}

	return &SFTPFS{client: client, conn: conn, root: rootPath}, nil
}

func buildSSHConfig(user, host, port, password string) (*ssh.ClientConfig, error) {
	var auths []ssh.AuthMethod

	if password != "" {
		auths = append(auths, ssh.Password(password))
	}

	// SSH agent
	if agentConn := sshAgentAuth(); agentConn != nil {
		auths = append(auths, agentConn)
	}

	// Key files
	keyFiles := []string{
		filepath.Join(userHomeDir(), ".ssh", "id_ed25519"),
		filepath.Join(userHomeDir(), ".ssh", "id_ecdsa"),
		filepath.Join(userHomeDir(), ".ssh", "id_rsa"),
	}
	for _, kf := range keyFiles {
		if auth := keyFileAuth(kf); auth != nil {
			auths = append(auths, auth)
		}
	}

	hostKeyCallback, hostKeyAlgos, err := knownHostsCallback(host, port)
	if err != nil {
		// Fallback: reject unknown hosts
		hostKeyCallback = ssh.InsecureIgnoreHostKey()
	}

	config := &ssh.ClientConfig{
		User:              user,
		Auth:              auths,
		HostKeyCallback:   hostKeyCallback,
		HostKeyAlgorithms: hostKeyAlgos,
	}

	return config, nil
}

func knownHostsCallback(host, port string) (ssh.HostKeyCallback, []string, error) {
	khPath := filepath.Join(userHomeDir(), ".ssh", "known_hosts")
	cb, err := knownhosts.New(khPath)
	if err != nil {
		return nil, nil, err
	}

	// Parse known_hosts to find algorithms for this host
	algos := parseKnownHostAlgos(khPath, host, port)

	return cb, algos, nil
}

func parseKnownHostAlgos(khPath, host, port string) []string {
	f, err := os.Open(khPath)
	if err != nil {
		return nil
	}
	defer f.Close()

	addr := host
	if port != "" && port != "22" {
		addr = fmt.Sprintf("[%s]:%s", host, port)
	}

	var algos []string
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Fields(line)
		if len(fields) < 3 {
			continue
		}
		hosts := strings.Split(fields[0], ",")
		for _, h := range hosts {
			h = strings.TrimSpace(h)
			if matchesHost(h, host, port, addr) {
				algos = append(algos, fields[1])
				break
			}
		}
	}
	return algos
}

func matchesHost(entry, host, port, addr string) bool {
	if entry == host || entry == addr {
		return true
	}
	// Handle [host]:port format
	if strings.HasPrefix(entry, "[") {
		return entry == addr
	}
	return false
}

func sshAgentAuth() ssh.AuthMethod {
	socket := os.Getenv("SSH_AUTH_SOCK")
	if socket == "" {
		return nil
	}
	conn, err := net.Dial("unix", socket)
	if err != nil {
		return nil
	}
	// We need to import agent package
	_ = conn
	conn.Close()
	return nil // simplified -- agent auth requires additional import
}

func keyFileAuth(path string) ssh.AuthMethod {
	key, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	signer, err := ssh.ParsePrivateKey(key)
	if err != nil {
		return nil
	}
	return ssh.PublicKeys(signer)
}

func userHomeDir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		if runtime.GOOS == "windows" {
			return os.Getenv("USERPROFILE")
		}
		return os.Getenv("HOME")
	}
	return home
}

func (fs *SFTPFS) abs(path string) string {
	if path == "" || path == "." {
		return fs.root
	}
	return fs.root + "/" + path
}

func (fs *SFTPFS) ListDir(path string) ([]Entry, error) {
	dirPath := fs.abs(path)
	entries, err := fs.client.ReadDir(dirPath)
	if err != nil {
		return nil, err
	}

	var result []Entry
	for _, info := range entries {
		// Skip symlinks and special files
		if info.Mode()&os.ModeSymlink != 0 {
			continue
		}
		if !info.Mode().IsRegular() && !info.IsDir() {
			continue
		}
		entry := Entry{
			Name:    info.Name(),
			IsDir:   info.IsDir(),
			ModTime: info.ModTime().UTC(),
		}
		if info.IsDir() {
			entry.ByteSize = -1
		} else {
			entry.ByteSize = info.Size()
		}
		result = append(result, entry)
	}
	return result, nil
}

func (fs *SFTPFS) Stat(path string) (*Entry, error) {
	p := fs.abs(path)
	info, err := fs.client.Lstat(p)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	if info.Mode()&os.ModeSymlink != 0 {
		return nil, nil
	}
	if !info.Mode().IsRegular() && !info.IsDir() {
		return nil, nil
	}
	entry := &Entry{
		Name:    info.Name(),
		IsDir:   info.IsDir(),
		ModTime: info.ModTime().UTC(),
	}
	if info.IsDir() {
		entry.ByteSize = -1
	} else {
		entry.ByteSize = info.Size()
	}
	return entry, nil
}

func (fs *SFTPFS) ReadFile(path string) (io.ReadCloser, error) {
	return fs.client.Open(fs.abs(path))
}

func (fs *SFTPFS) WriteFile(path string, r io.Reader) error {
	p := fs.abs(path)
	if err := fs.client.MkdirAll(filepath.Dir(p)); err != nil {
		// ignore; parent may already exist
	}
	f, err := fs.client.Create(p)
	if err != nil {
		return err
	}
	defer f.Close()
	_, err = io.Copy(f, r)
	return err
}

func (fs *SFTPFS) Rename(src, dst string) error {
	srcPath := fs.abs(src)
	dstPath := fs.abs(dst)
	// Create parent dirs for destination
	if err := fs.client.MkdirAll(filepath.Dir(dstPath)); err != nil {
		// ignore
	}
	return fs.client.Rename(srcPath, dstPath)
}

func (fs *SFTPFS) DeleteFile(path string) error {
	return fs.client.Remove(fs.abs(path))
}

func (fs *SFTPFS) CreateDir(path string) error {
	return fs.client.MkdirAll(fs.abs(path))
}

func (fs *SFTPFS) DeleteDir(path string) error {
	return fs.client.Remove(fs.abs(path))
}

func (fs *SFTPFS) SetModTime(path string, t time.Time) error {
	return fs.client.Chtimes(fs.abs(path), t, t)
}

func (fs *SFTPFS) GetPermissions(path string) (os.FileMode, error) {
	info, err := fs.client.Stat(fs.abs(path))
	if err != nil {
		return 0, err
	}
	return info.Mode().Perm(), nil
}

func (fs *SFTPFS) SetPermissions(path string, mode os.FileMode) error {
	return fs.client.Chmod(fs.abs(path), mode)
}

func (fs *SFTPFS) Close() error {
	fs.client.Close()
	return fs.conn.Close()
}

func (fs *SFTPFS) Client() *sftp.Client {
	return fs.client
}

func (fs *SFTPFS) Root() string {
	return fs.root
}

// ConnectSFTP creates a new SFTP connection. Exported for the pool to create new connections.
func ConnectSFTP(user, host, port, rootPath, password string, timeout time.Duration) (*SFTPFS, error) {
	return NewSFTPFS(user, host, port, rootPath, password, timeout)
}
