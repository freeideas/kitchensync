package lock

import (
	"encoding/json"
	"fmt"
	"io"
	"kitchensync/internal/logx"
	"net"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"
)

type InstanceLock struct {
	listener net.Listener
	port     int
	peers    []string
	mu       sync.Mutex
	shutdown chan struct{}
	server   *http.Server
}

func New(peers []string) *InstanceLock {
	return &InstanceLock{
		peers:    peers,
		shutdown: make(chan struct{}),
	}
}

func (l *InstanceLock) Bind() error {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return fmt.Errorf("instance lock: %w", err)
	}
	l.listener = ln
	l.port = ln.Addr().(*net.TCPAddr).Port

	mux := http.NewServeMux()
	mux.HandleFunc("/instance-peers", l.handlePeers)
	mux.HandleFunc("/shutdown", l.handleShutdown)

	l.server = &http.Server{Handler: mux}
	go l.server.Serve(ln)

	return nil
}

func (l *InstanceLock) Port() int {
	return l.port
}

func (l *InstanceLock) ShutdownChan() <-chan struct{} {
	return l.shutdown
}

func (l *InstanceLock) handlePeers(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(l.peers)
}

func (l *InstanceLock) handleShutdown(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	w.WriteHeader(http.StatusOK)
	w.Write([]byte("ok"))
	go func() {
		close(l.shutdown)
	}()
}

func (l *InstanceLock) Close() error {
	if l.server != nil {
		l.server.Close()
	}
	if l.listener != nil {
		l.listener.Close()
	}
	return nil
}

// CheckExisting checks if any existing instance has overlapping peers.
// Returns an error if overlap is detected. Stale locks return the port for cleanup.
func CheckExisting(portStr string, ourPeers []string) (bool, error) {
	portStr = strings.TrimSpace(portStr)
	if portStr == "" {
		return false, nil
	}
	port, err := strconv.Atoi(portStr)
	if err != nil {
		return false, nil // garbage, treat as no lock
	}

	client := &http.Client{Timeout: 5 * time.Second}
	resp, err := client.Post(fmt.Sprintf("http://127.0.0.1:%d/instance-peers", port), "", nil)
	if err != nil {
		// Connection refused = stale lock
		return false, nil
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return false, nil
	}

	var existingPeers []string
	if err := json.Unmarshal(body, &existingPeers); err != nil {
		return false, nil
	}

	// Check overlap
	ourSet := make(map[string]bool)
	for _, p := range ourPeers {
		ourSet[p] = true
	}
	for _, ep := range existingPeers {
		if ourSet[ep] {
			return true, fmt.Errorf("overlapping instance on port %d, peer: %s", port, ep)
		}
	}

	return false, nil
}

func WriteLockFile(writeFunc func(path string, data []byte) error, peerRoot string, port int) error {
	lockPath := ".kitchensync/lock"
	data := []byte(strconv.Itoa(port))
	return writeFunc(lockPath, data)
}

func ReadLockFile(readFunc func(path string) ([]byte, error), peerRoot string) string {
	lockPath := ".kitchensync/lock"
	data, err := readFunc(lockPath)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

func DeleteLockFile(deleteFunc func(path string) error, peerRoot string) {
	lockPath := ".kitchensync/lock"
	if err := deleteFunc(lockPath); err != nil {
		logx.Debug("failed to delete lock file: %v", err)
	}
}
