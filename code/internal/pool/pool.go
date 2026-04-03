package pool

import (
	"fmt"
	"kitchensync/internal/fsys"
	"kitchensync/internal/logx"
	"kitchensync/internal/urlnorm"
	"net/url"
	"os"
	"runtime"
	"strings"
	"sync"
	"time"
)

type ConnPool struct {
	mu       sync.Mutex
	conns    []fsys.PeerFS
	max      int
	active   int
	url      string
	timeout  time.Duration
	password string
	sem      chan struct{}
}

type Manager struct {
	mu    sync.Mutex
	pools map[string]*ConnPool
}

func NewManager() *Manager {
	return &Manager{pools: make(map[string]*ConnPool)}
}

func (m *Manager) GetOrCreate(normalizedURL string, mc int, ct int, password string) *ConnPool {
	m.mu.Lock()
	defer m.mu.Unlock()
	if p, ok := m.pools[normalizedURL]; ok {
		return p
	}
	p := &ConnPool{
		max:      mc,
		url:      normalizedURL,
		timeout:  time.Duration(ct) * time.Second,
		password: password,
		sem:      make(chan struct{}, mc),
	}
	m.pools[normalizedURL] = p
	return p
}

func (m *Manager) CloseAll() {
	m.mu.Lock()
	defer m.mu.Unlock()
	for _, p := range m.pools {
		p.mu.Lock()
		for _, c := range p.conns {
			c.Close()
		}
		p.conns = nil
		p.mu.Unlock()
	}
}

func (p *ConnPool) Acquire() (fsys.PeerFS, error) {
	p.sem <- struct{}{} // block until slot available

	p.mu.Lock()
	if len(p.conns) > 0 {
		conn := p.conns[len(p.conns)-1]
		p.conns = p.conns[:len(p.conns)-1]
		p.active++
		p.mu.Unlock()
		logx.Trace("url=%s connections=%d/%d", p.url, p.active, p.max)
		return conn, nil
	}
	p.active++
	p.mu.Unlock()

	conn, err := p.createConn()
	if err != nil {
		p.mu.Lock()
		p.active--
		p.mu.Unlock()
		<-p.sem
		return nil, err
	}
	logx.Trace("url=%s connections=%d/%d", p.url, p.active, p.max)
	return conn, nil
}

func (p *ConnPool) Release(conn fsys.PeerFS) {
	p.mu.Lock()
	p.conns = append(p.conns, conn)
	p.active--
	p.mu.Unlock()
	<-p.sem
	logx.Trace("url=%s connections=%d/%d", p.url, p.active, p.max)
}

func (p *ConnPool) Remove(conn fsys.PeerFS) {
	conn.Close()
	p.mu.Lock()
	p.active--
	p.mu.Unlock()
	<-p.sem
}

func (p *ConnPool) createConn() (fsys.PeerFS, error) {
	scheme := urlnorm.Scheme(p.url)
	if scheme == "file" {
		osPath := urlnorm.OSPath(p.url)
		return fsys.NewLocalFS(osPath), nil
	}
	if scheme == "sftp" {
		user, host, port, path := urlnorm.ParseSFTP(p.url)
		if user == "" {
			user = currentUser()
		}
		return fsys.ConnectSFTP(user, host, port, path, p.password, p.timeout)
	}
	return nil, fmt.Errorf("unsupported scheme: %s", p.url)
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

// AcquireOrdered acquires connections from two pools in lexicographic URL order to prevent deadlock.
func AcquireOrdered(p1, p2 *ConnPool) (fsys.PeerFS, fsys.PeerFS, error) {
	if p1.url <= p2.url {
		c1, err := p1.Acquire()
		if err != nil {
			return nil, nil, err
		}
		c2, err := p2.Acquire()
		if err != nil {
			p1.Release(c1)
			return nil, nil, err
		}
		return c1, c2, nil
	}
	c2, err := p2.Acquire()
	if err != nil {
		return nil, nil, err
	}
	c1, err := p1.Acquire()
	if err != nil {
		p2.Release(c2)
		return nil, nil, err
	}
	return c1, c2, nil
}

// ExtractPassword extracts inline password from a raw sftp:// URL.
func ExtractPassword(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil {
		return ""
	}
	if u.User == nil {
		return ""
	}
	pw, _ := u.User.Password()
	return pw
}

func init() {
	_ = runtime.GOOS
	_ = strings.TrimSpace
}
