package db

import (
	"database/sql"
	"fmt"
	"kitchensync/internal/hash"
	"kitchensync/internal/ts"
	"os"
	"path/filepath"
	"time"

	_ "modernc.org/sqlite"
)

const SentinelPath = "/"

type SnapshotDB struct {
	db   *sql.DB
	path string
}

type Row struct {
	ID          string
	ParentID    string
	Basename    string
	ModTime     string
	ByteSize    int64
	LastSeen    sql.NullString
	DeletedTime sql.NullString
}

func Open(dbPath string) (*SnapshotDB, error) {
	if err := os.MkdirAll(filepath.Dir(dbPath), 0755); err != nil {
		return nil, err
	}
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	if _, err := db.Exec("PRAGMA journal_mode=WAL"); err != nil {
		db.Close()
		return nil, err
	}
	if _, err := db.Exec("PRAGMA busy_timeout=5000"); err != nil {
		db.Close()
		return nil, err
	}
	if _, err := db.Exec("PRAGMA foreign_keys=ON"); err != nil {
		db.Close()
		return nil, err
	}
	return &SnapshotDB{db: db, path: dbPath}, nil
}

func (s *SnapshotDB) Init() error {
	schema := `
CREATE TABLE IF NOT EXISTS snapshot (
    id           TEXT PRIMARY KEY,
    parent_id    TEXT NOT NULL,
    basename     TEXT NOT NULL,
    mod_time     TEXT NOT NULL,
    byte_size    INTEGER NOT NULL,
    last_seen    TEXT,
    deleted_time TEXT,
    FOREIGN KEY (parent_id) REFERENCES snapshot(id)
);
CREATE INDEX IF NOT EXISTS idx_parent_id ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_last_seen ON snapshot(last_seen);
CREATE INDEX IF NOT EXISTS idx_deleted_time ON snapshot(deleted_time);
`
	if _, err := s.db.Exec(schema); err != nil {
		return err
	}

	// Insert sentinel if not exists
	sentinelID := hash.PathID(SentinelPath)
	_, err := s.db.Exec(
		`INSERT OR IGNORE INTO snapshot (id, parent_id, basename, mod_time, byte_size)
		 VALUES (?, ?, '', '0000-00-00_00-00-00_000000Z', -1)`,
		sentinelID, sentinelID,
	)
	return err
}

func (s *SnapshotDB) Close() error {
	return s.db.Close()
}

func (s *SnapshotDB) Path() string {
	return s.path
}

func (s *SnapshotDB) DB() *sql.DB {
	return s.db
}

func (s *SnapshotDB) Checkpoint() error {
	_, err := s.db.Exec("PRAGMA wal_checkpoint(TRUNCATE)")
	return err
}

func (s *SnapshotDB) Lookup(relPath string) (*Row, error) {
	id := hash.PathID(relPath)
	row := s.db.QueryRow(
		`SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
		 FROM snapshot WHERE id = ?`, id)
	r := &Row{}
	err := row.Scan(&r.ID, &r.ParentID, &r.Basename, &r.ModTime, &r.ByteSize,
		&r.LastSeen, &r.DeletedTime)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return r, nil
}

func (s *SnapshotDB) LookupByID(id string) (*Row, error) {
	row := s.db.QueryRow(
		`SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
		 FROM snapshot WHERE id = ?`, id)
	r := &Row{}
	err := row.Scan(&r.ID, &r.ParentID, &r.Basename, &r.ModTime, &r.ByteSize,
		&r.LastSeen, &r.DeletedTime)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return r, nil
}

func (s *SnapshotDB) Upsert(relPath, parentRelPath, basename, modTime string, byteSize int64, lastSeen *string, deletedTime *string) error {
	id := hash.PathID(relPath)
	parentID := hash.PathID(parentRelPath)

	var ls, dt sql.NullString
	if lastSeen != nil {
		ls = sql.NullString{String: *lastSeen, Valid: true}
	}
	if deletedTime != nil {
		dt = sql.NullString{String: *deletedTime, Valid: true}
	}

	_, err := s.db.Exec(`
		INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
		VALUES (?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			parent_id = excluded.parent_id,
			basename = excluded.basename,
			mod_time = excluded.mod_time,
			byte_size = excluded.byte_size,
			last_seen = excluded.last_seen,
			deleted_time = excluded.deleted_time`,
		id, parentID, basename, modTime, byteSize, ls, dt)
	return err
}

func (s *SnapshotDB) SetDeletedTime(relPath string, deletedTime string) error {
	id := hash.PathID(relPath)
	_, err := s.db.Exec(
		`UPDATE snapshot SET deleted_time = ? WHERE id = ? AND deleted_time IS NULL`,
		deletedTime, id)
	return err
}

func (s *SnapshotDB) SetLastSeen(relPath string, lastSeen string) error {
	id := hash.PathID(relPath)
	_, err := s.db.Exec(
		`UPDATE snapshot SET last_seen = ? WHERE id = ?`,
		lastSeen, id)
	return err
}

func (s *SnapshotDB) CascadeTombstones(relPath string, deletedTime string) error {
	id := hash.PathID(relPath)
	_, err := s.db.Exec(`
		WITH RECURSIVE subtree(id) AS (
			VALUES(?)
			UNION ALL
			SELECT s.id FROM snapshot s
			JOIN subtree st ON s.parent_id = st.id
		)
		UPDATE snapshot
		SET deleted_time = ?
		WHERE id IN (SELECT id FROM subtree)`,
		id, deletedTime)
	return err
}

func (s *SnapshotDB) PurgeTombstones(tdDays int) error {
	if tdDays <= 0 {
		return nil
	}
	cutoff := time.Now().UTC().AddDate(0, 0, -tdDays)
	cutoffStr := ts.Format(cutoff)

	// Purge old tombstones
	_, err := s.db.Exec(
		`DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?`,
		cutoffStr)
	if err != nil {
		return err
	}

	// Purge stale non-tombstone rows (last_seen too old, but not NULL)
	_, err = s.db.Exec(
		`DELETE FROM snapshot WHERE deleted_time IS NULL AND last_seen IS NOT NULL AND last_seen < ? AND id != parent_id`,
		cutoffStr)
	return err
}

func (s *SnapshotDB) HasRows() (bool, error) {
	sentinelID := hash.PathID(SentinelPath)
	var count int
	err := s.db.QueryRow(
		`SELECT COUNT(*) FROM snapshot WHERE id != ?`, sentinelID).Scan(&count)
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

func (s *SnapshotDB) ChildrenOf(parentRelPath string) ([]*Row, error) {
	parentID := hash.PathID(parentRelPath)
	rows, err := s.db.Query(
		`SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
		 FROM snapshot WHERE parent_id = ? AND id != parent_id`, parentID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var result []*Row
	for rows.Next() {
		r := &Row{}
		if err := rows.Scan(&r.ID, &r.ParentID, &r.Basename, &r.ModTime, &r.ByteSize,
			&r.LastSeen, &r.DeletedTime); err != nil {
			return nil, err
		}
		result = append(result, r)
	}
	return result, rows.Err()
}

func RelPath(parentPath, basename string) string {
	if parentPath == SentinelPath || parentPath == "" {
		return basename
	}
	return parentPath + "/" + basename
}

func ParentPath(relPath string) string {
	idx := len(relPath) - 1
	for idx >= 0 && relPath[idx] != '/' {
		idx--
	}
	if idx < 0 {
		return SentinelPath
	}
	return relPath[:idx]
}

func Basename(relPath string) string {
	idx := len(relPath) - 1
	for idx >= 0 && relPath[idx] != '/' {
		idx--
	}
	return relPath[idx+1:]
}

func FormatNullString(ns sql.NullString) *string {
	if !ns.Valid {
		return nil
	}
	return &ns.String
}

func StringPtr(s string) *string {
	return &s
}

func ParseModTime(s string) (time.Time, error) {
	return ts.Parse(s)
}

func FormatModTime(t time.Time) string {
	return ts.Format(t)
}

func init() {
	// Verify fmt is used
	_ = fmt.Sprintf
}
