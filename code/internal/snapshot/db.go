package snapshot

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"kitchensync/internal/hash"
	"kitchensync/internal/timestamp"

	_ "modernc.org/sqlite"
)

// DB wraps a SQLite snapshot database.
type DB struct {
	db   *sql.DB
	path string
}

// Row represents a snapshot row.
type Row struct {
	ID          string
	ParentID    string
	Basename    string
	ModTime     string
	ByteSize    int64
	LastSeen    sql.NullString
	DeletedTime sql.NullString
}

// Open opens or creates a snapshot database at the given path.
func Open(dbPath string) (*DB, error) {
	if err := os.MkdirAll(filepath.Dir(dbPath), 0755); err != nil {
		return nil, err
	}

	db, err := sql.Open("sqlite", dbPath+"?_pragma=journal_mode(WAL)&_pragma=foreign_keys(ON)&_pragma=busy_timeout(5000)")
	if err != nil {
		return nil, err
	}

	if err := createSchema(db); err != nil {
		db.Close()
		return nil, err
	}

	return &DB{db: db, path: dbPath}, nil
}

func createSchema(db *sql.DB) error {
	_, err := db.Exec(`
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
	`)
	if err != nil {
		return err
	}

	// Insert sentinel if not exists
	sentinelID := hash.SentinelID()
	_, err = db.Exec(`INSERT OR IGNORE INTO snapshot (id, parent_id, basename, mod_time, byte_size) VALUES (?, ?, '', '0000-00-00_00-00-00_000000Z', -1)`,
		sentinelID, sentinelID)
	return err
}

// Close closes the database.
func (d *DB) Close() error { return d.db.Close() }

// Path returns the database file path.
func (d *DB) Path() string { return d.path }

// Get retrieves a snapshot row by path.
func (d *DB) Get(relPath string) (*Row, error) {
	id := hash.PathID(relPath)
	row := d.db.QueryRow(`SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot WHERE id = ?`, id)
	r := &Row{}
	err := row.Scan(&r.ID, &r.ParentID, &r.Basename, &r.ModTime, &r.ByteSize, &r.LastSeen, &r.DeletedTime)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return r, nil
}

// Upsert inserts or updates a snapshot row.
func (d *DB) Upsert(relPath, parentPath, basename, modTime string, byteSize int64, lastSeen *string, deletedTime *string) error {
	id := hash.PathID(relPath)
	parentID := hash.PathID(parentPath)
	if parentPath == "" {
		parentID = hash.SentinelID()
	}

	var ls, dt sql.NullString
	if lastSeen != nil {
		ls = sql.NullString{String: *lastSeen, Valid: true}
	}
	if deletedTime != nil {
		dt = sql.NullString{String: *deletedTime, Valid: true}
	}

	_, err := d.db.Exec(`
		INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
		VALUES (?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			parent_id = excluded.parent_id,
			basename = excluded.basename,
			mod_time = excluded.mod_time,
			byte_size = excluded.byte_size,
			last_seen = excluded.last_seen,
			deleted_time = excluded.deleted_time
	`, id, parentID, basename, modTime, byteSize, ls, dt)
	return err
}

// SetLastSeen updates only the last_seen field for a path.
func (d *DB) SetLastSeen(relPath string, lastSeen string) error {
	id := hash.PathID(relPath)
	_, err := d.db.Exec(`UPDATE snapshot SET last_seen = ? WHERE id = ?`, lastSeen, id)
	return err
}

// SetDeletedTime sets deleted_time from last_seen for an entry.
func (d *DB) SetDeletedTime(relPath string) error {
	id := hash.PathID(relPath)
	_, err := d.db.Exec(`UPDATE snapshot SET deleted_time = last_seen WHERE id = ? AND deleted_time IS NULL`, id)
	return err
}

// SetDeletedTimeValue sets deleted_time to a specific value.
func (d *DB) SetDeletedTimeValue(relPath string, deletedTime string) error {
	id := hash.PathID(relPath)
	_, err := d.db.Exec(`UPDATE snapshot SET deleted_time = ? WHERE id = ? AND deleted_time IS NULL`, deletedTime, id)
	return err
}

// CascadeTombstones marks all descendants of a path as deleted.
func (d *DB) CascadeTombstones(relPath string, deletedTime string) error {
	id := hash.PathID(relPath)
	_, err := d.db.Exec(`
		WITH RECURSIVE subtree(id) AS (
			VALUES(?)
			UNION ALL
			SELECT s.id FROM snapshot s
			JOIN subtree st ON s.parent_id = st.id
			WHERE s.deleted_time IS NULL
		)
		UPDATE snapshot
		SET deleted_time = ?
		WHERE deleted_time IS NULL
		AND id IN (SELECT id FROM subtree)
	`, id, deletedTime)
	return err
}

// PurgeTombstones deletes rows where deleted_time is older than maxAge days.
func (d *DB) PurgeTombstones(maxAgeDays int) error {
	cutoff := time.Now().UTC().AddDate(0, 0, -maxAgeDays)
	cutoffStr := timestamp.FormatTime(cutoff)
	_, err := d.db.Exec(`DELETE FROM snapshot WHERE deleted_time IS NOT NULL AND deleted_time < ?`, cutoffStr)
	if err != nil {
		return err
	}
	// Also purge stale non-tombstone rows that haven't been seen in maxAgeDays
	_, err = d.db.Exec(`DELETE FROM snapshot WHERE deleted_time IS NULL AND last_seen IS NOT NULL AND last_seen < ? AND id != ?`,
		cutoffStr, hash.SentinelID())
	return err
}

// HasData returns true if the database has any non-sentinel rows.
func (d *DB) HasData() (bool, error) {
	sentinelID := hash.SentinelID()
	var count int
	err := d.db.QueryRow(`SELECT COUNT(*) FROM snapshot WHERE id != ?`, sentinelID).Scan(&count)
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

// GetChildren returns snapshot rows whose parent is the given path.
func (d *DB) GetChildren(parentPath string) ([]Row, error) {
	var parentID string
	if parentPath == "" || parentPath == "/" {
		parentID = hash.SentinelID()
	} else {
		parentID = hash.PathID(parentPath)
	}

	rows, err := d.db.Query(`SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot WHERE parent_id = ? AND id != ?`,
		parentID, parentID) // exclude sentinel self-reference
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var result []Row
	for rows.Next() {
		var r Row
		if err := rows.Scan(&r.ID, &r.ParentID, &r.Basename, &r.ModTime, &r.ByteSize, &r.LastSeen, &r.DeletedTime); err != nil {
			return nil, err
		}
		result = append(result, r)
	}
	return result, rows.Err()
}

// UpsertWithNullLastSeen inserts/updates a row leaving last_seen NULL (for pending copies).
func (d *DB) UpsertWithNullLastSeen(relPath, parentPath, basename, modTime string, byteSize int64) error {
	id := hash.PathID(relPath)
	parentID := hash.PathID(parentPath)
	if parentPath == "" {
		parentID = hash.SentinelID()
	}

	_, err := d.db.Exec(`
		INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
		VALUES (?, ?, ?, ?, ?, NULL, NULL)
		ON CONFLICT(id) DO UPDATE SET
			parent_id = excluded.parent_id,
			basename = excluded.basename,
			mod_time = excluded.mod_time,
			byte_size = excluded.byte_size,
			deleted_time = NULL
	`, id, parentID, basename, modTime, byteSize)
	return err
}

// ParentPath returns the parent directory of a relative path.
func ParentPath(relPath string) string {
	for i := len(relPath) - 1; i >= 0; i-- {
		if relPath[i] == '/' {
			return relPath[:i]
		}
	}
	return ""
}

// BaseName returns the final path component.
func BaseName(relPath string) string {
	for i := len(relPath) - 1; i >= 0; i-- {
		if relPath[i] == '/' {
			return relPath[i+1:]
		}
	}
	return relPath
}

// ParseModTime parses a mod_time string to time.Time.
func ParseModTime(s string) (time.Time, error) {
	return timestamp.ParseTime(s)
}

// FormatModTime formats a time for the mod_time column.
func FormatModTime(t time.Time) string {
	return timestamp.FormatTime(t)
}

// Exec runs arbitrary SQL (for testing).
func (d *DB) Exec(query string, args ...any) (sql.Result, error) {
	return d.db.Exec(query, args...)
}

// Query runs a query (for testing).
func (d *DB) Query(query string, args ...any) (*sql.Rows, error) {
	return d.db.Query(query, args...)
}

// GetRaw returns raw fields for debugging.
func (d *DB) GetRaw(relPath string) (modTime string, byteSize int64, lastSeen, deletedTime sql.NullString, found bool, err error) {
	id := hash.PathID(relPath)
	row := d.db.QueryRow(`SELECT mod_time, byte_size, last_seen, deleted_time FROM snapshot WHERE id = ?`, id)
	err = row.Scan(&modTime, &byteSize, &lastSeen, &deletedTime)
	if err == sql.ErrNoRows {
		return "", 0, sql.NullString{}, sql.NullString{}, false, nil
	}
	if err != nil {
		return "", 0, sql.NullString{}, sql.NullString{}, false, err
	}
	return modTime, byteSize, lastSeen, deletedTime, true, nil
}

// Count returns the total number of rows excluding the sentinel.
func (d *DB) Count() (int, error) {
	var count int
	err := d.db.QueryRow(`SELECT COUNT(*) FROM snapshot WHERE id != ?`, hash.SentinelID()).Scan(&count)
	return count, err
}

// DeleteByPath deletes a row by its relative path.
func (d *DB) DeleteByPath(relPath string) error {
	id := hash.PathID(relPath)
	_, err := d.db.Exec(`DELETE FROM snapshot WHERE id = ?`, id)
	return err
}

func strPtr(s string) *string { return &s }

// Helpers exported for convenience.
func StrPtr(s string) *string { return &s }
func NowStr() string          { return timestamp.FormatTime(timestamp.Now()) }

// WithinTolerance checks if two times are within 5 seconds of each other.
func WithinTolerance(a, b time.Time) bool {
	diff := a.Sub(b)
	if diff < 0 {
		diff = -diff
	}
	return diff <= 5*time.Second
}

// FormatNow returns a formatted current timestamp.
func FormatNow() string {
	return fmt.Sprintf("%s", NowStr())
}
