package ts

import (
	"fmt"
	"sync"
	"time"
)

var (
	mu   sync.Mutex
	last time.Time
)

// Now returns a monotonic UTC timestamp in KitchenSync format.
// Adds 1us on collision to guarantee uniqueness within a process.
func Now() string {
	mu.Lock()
	defer mu.Unlock()
	t := time.Now().UTC()
	if !t.After(last) {
		t = last.Add(time.Microsecond)
	}
	last = t
	return Format(t)
}

// Format converts a time.Time to KitchenSync timestamp format.
func Format(t time.Time) string {
	t = t.UTC()
	return fmt.Sprintf("%04d-%02d-%02d_%02d-%02d-%02d_%06dZ",
		t.Year(), t.Month(), t.Day(),
		t.Hour(), t.Minute(), t.Second(),
		t.Nanosecond()/1000)
}

// Parse parses a KitchenSync timestamp string.
func Parse(s string) (time.Time, error) {
	if len(s) != 27 {
		return time.Time{}, fmt.Errorf("invalid timestamp length: %q", s)
	}
	var year, month, day, hour, min, sec, usec int
	_, err := fmt.Sscanf(s, "%04d-%02d-%02d_%02d-%02d-%02d_%06dZ",
		&year, &month, &day, &hour, &min, &sec, &usec)
	if err != nil {
		return time.Time{}, fmt.Errorf("invalid timestamp %q: %w", s, err)
	}
	return time.Date(year, time.Month(month), day, hour, min, sec, usec*1000, time.UTC), nil
}
