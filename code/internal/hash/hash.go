package hash

import (
	"github.com/cespare/xxhash/v2"
)

const alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

// PathID returns the xxHash64 of the given path, base62-encoded to 11 chars.
func PathID(path string) string {
	h := xxhash.Sum64String(path)
	return Base62(h)
}

// Base62 encodes a uint64 as an 11-character base62 string, zero-padded, most-significant digit first.
func Base62(n uint64) string {
	buf := make([]byte, 11)
	for i := 10; i >= 0; i-- {
		buf[i] = alphabet[n%62]
		n /= 62
	}
	return string(buf)
}
