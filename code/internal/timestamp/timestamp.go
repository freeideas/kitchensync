package timestamp

import (
	"fmt"
	"sync"
	"time"
)

// Format: YYYY-MM-DD_HH-mm-ss_ffffffZ
const Format = "2006-01-02_15-04-05_000000Z"

var (
	mu   sync.Mutex
	last time.Time
)

// Now returns a monotonic UTC timestamp. Adds 1us on collision.
func Now() time.Time {
	mu.Lock()
	defer mu.Unlock()
	t := time.Now().UTC()
	if !t.After(last) {
		t = last.Add(time.Microsecond)
	}
	last = t
	return t
}

// FormatTime formats a time in the KitchenSync timestamp format.
func FormatTime(t time.Time) string {
	u := t.UTC()
	return fmt.Sprintf("%04d-%02d-%02d_%02d-%02d-%02d_%06dZ",
		u.Year(), u.Month(), u.Day(),
		u.Hour(), u.Minute(), u.Second(),
		u.Nanosecond()/1000)
}

// ParseTime parses a KitchenSync timestamp string.
func ParseTime(s string) (time.Time, error) {
	var year, month, day, hour, min, sec, usec int
	_, err := fmt.Sscanf(s, "%04d-%02d-%02d_%02d-%02d-%02d_%06dZ",
		&year, &month, &day, &hour, &min, &sec, &usec)
	if err != nil {
		return time.Time{}, fmt.Errorf("invalid timestamp %q: %w", s, err)
	}
	return time.Date(year, time.Month(month), day, hour, min, sec, usec*1000, time.UTC), nil
}
