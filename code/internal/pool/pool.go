package pool

import (
	"fmt"
	"sort"
	"sync"
	"time"

	"kitchensync/internal/fsutil"
	"kitchensync/internal/log"
	"kitchensync/internal/urlutil"
)

// ConnPool manages a pool of filesystem connections for a single URL.
type ConnPool struct {
	mu       sync.Mutex
	url      *urlutil.NormalizedURL
	maxConn  int
	timeout  time.Duration
	conns    chan fsutil.PeerFS
	active   int
	identity string
}

// NewPool creates a connection pool for the given URL.
func NewPool(u *urlutil.NormalizedURL, globalMC int, globalCT int) *ConnPool {
	mc := globalMC
	if u.MaxConn > 0 {
		mc = u.MaxConn
	}
	ct := globalCT
	if u.ConnTimeout > 0 {
		ct = u.ConnTimeout
	}
	return &ConnPool{
		url:      u,
		maxConn:  mc,
		timeout:  time.Duration(ct) * time.Second,
		conns:    make(chan fsutil.PeerFS, mc),
		identity: u.Identity(),
	}
}

// Identity returns the pool key.
func (p *ConnPool) Identity() string { return p.identity }

// MaxConn returns the max connections.
func (p *ConnPool) MaxConn() int { return p.maxConn }

// Acquire gets a connection from the pool, creating one if under the limit.
func (p *ConnPool) Acquire() (fsutil.PeerFS, error) {
	// Try to get an existing idle connection
	select {
	case conn := <-p.conns:
		p.mu.Lock()
		p.active++
		log.Trace("url=%s connections=%d/%d", p.identity, p.active, p.maxConn)
		p.mu.Unlock()
		return conn, nil
	default:
	}

	p.mu.Lock()
	if p.active < p.maxConn {
		p.active++
		log.Trace("url=%s connections=%d/%d", p.identity, p.active, p.maxConn)
		p.mu.Unlock()
		conn, err := p.dial()
		if err != nil {
			p.mu.Lock()
			p.active--
			p.mu.Unlock()
			return nil, err
		}
		return conn, nil
	}
	p.mu.Unlock()

	// Wait for a connection to be returned
	conn := <-p.conns
	p.mu.Lock()
	p.active++
	log.Trace("url=%s connections=%d/%d", p.identity, p.active, p.maxConn)
	p.mu.Unlock()
	return conn, nil
}

// Release returns a connection to the pool.
func (p *ConnPool) Release(conn fsutil.PeerFS) {
	p.mu.Lock()
	p.active--
	log.Trace("url=%s connections=%d/%d", p.identity, p.active, p.maxConn)
	p.mu.Unlock()

	select {
	case p.conns <- conn:
	default:
		conn.Close()
	}
}

func (p *ConnPool) dial() (fsutil.PeerFS, error) {
	u := p.url
	switch u.Scheme {
	case "file":
		return fsutil.NewLocalFS(u.OSPath()), nil
	case "sftp":
		return fsutil.DialSFTP(u.User, u.Password, u.Host, u.Port, u.Path, p.timeout)
	default:
		return nil, fmt.Errorf("unsupported scheme: %s", u.Scheme)
	}
}

// Close closes all idle connections in the pool.
func (p *ConnPool) Close() {
	close(p.conns)
	for conn := range p.conns {
		conn.Close()
	}
}

// AcquireOrdered acquires connections from two pools in lexicographic URL order to prevent deadlock.
func AcquireOrdered(a, b *ConnPool) (fsutil.PeerFS, fsutil.PeerFS, error) {
	pools := []*ConnPool{a, b}
	sort.Slice(pools, func(i, j int) bool {
		return pools[i].Identity() < pools[j].Identity()
	})

	first, err := pools[0].Acquire()
	if err != nil {
		return nil, nil, err
	}
	second, err := pools[1].Acquire()
	if err != nil {
		pools[0].Release(first)
		return nil, nil, err
	}

	if pools[0] == a {
		return first, second, nil
	}
	return second, first, nil
}
