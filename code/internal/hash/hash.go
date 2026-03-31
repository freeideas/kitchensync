package hash

import (
	"github.com/cespare/xxhash/v2"
)

const alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

// PathID returns the xxHash64 of path, base62-encoded to 11 chars.
func PathID(path string) string {
	h := xxhash.Sum64String(path)
	return encodeBase62(h)
}

func encodeBase62(n uint64) string {
	buf := make([]byte, 11)
	for i := 10; i >= 0; i-- {
		buf[i] = alphabet[n%62]
		n /= 62
	}
	return string(buf)
}

// SentinelID returns the hash of "/" (the root sentinel).
func SentinelID() string {
	return PathID("/")
}
