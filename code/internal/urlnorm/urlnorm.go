package urlnorm

import (
	"fmt"
	"net/url"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
)

// Normalize normalizes a URL string per the spec:
// - Lowercase scheme and hostname
// - Remove default port (22 for SFTP)
// - Collapse consecutive slashes in path
// - Remove trailing slash from path
// - Bare paths -> file:// with absolute resolution
// - Percent-decode unreserved characters
// - Strip query-string parameters
func Normalize(raw string) string {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return ""
	}

	// Detect bare paths (no scheme)
	if isBarePathWindows(raw) || isBarePathUnix(raw) {
		return normalizeFilePath(raw)
	}

	// Check for scheme
	if !hasScheme(raw) {
		return normalizeFilePath(raw)
	}

	// Parse as URL
	u, err := url.Parse(raw)
	if err != nil {
		return raw
	}

	scheme := strings.ToLower(u.Scheme)

	if scheme == "file" {
		return normalizeFileURL(u)
	}

	if scheme == "sftp" {
		return normalizeSFTPURL(u)
	}

	return raw
}

func isBarePathWindows(s string) bool {
	if len(s) >= 2 && s[1] == ':' && isLetter(s[0]) {
		return true
	}
	// backslash path
	if len(s) > 0 && s[0] == '\\' {
		return true
	}
	return false
}

func isBarePathUnix(s string) bool {
	if len(s) > 0 && s[0] == '/' {
		return true
	}
	if len(s) > 0 && s[0] == '.' {
		return true
	}
	return false
}

func hasScheme(s string) bool {
	for i, c := range s {
		if c == ':' && i > 0 {
			// Check if next chars are //
			if i+2 < len(s) && s[i+1] == '/' && s[i+2] == '/' {
				return true
			}
			// Could be a Windows drive letter
			if i == 1 && isLetter(byte(s[0])) {
				return false
			}
			return false
		}
		if !isSchemeChar(c, i == 0) {
			return false
		}
	}
	return false
}

func isSchemeChar(c rune, first bool) bool {
	if c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' {
		return true
	}
	if first {
		return false
	}
	return c >= '0' && c <= '9' || c == '+' || c == '-' || c == '.'
}

func isLetter(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
}

func normalizeFilePath(raw string) string {
	// Convert backslashes to forward slashes
	p := strings.ReplaceAll(raw, "\\", "/")

	// Resolve to absolute
	abs, err := filepath.Abs(strings.ReplaceAll(raw, "/", string(filepath.Separator)))
	if err != nil {
		abs = raw
	}
	// Resolve symlinks
	resolved, err := filepath.EvalSymlinks(abs)
	if err != nil {
		resolved = abs
	}
	p = filepath.ToSlash(resolved)

	// On Windows, ensure drive letter path like c:/photos
	if runtime.GOOS == "windows" && len(p) >= 2 && p[1] == ':' {
		p = strings.ToLower(p[:1]) + p[1:]
	}

	p = collapseSlashes(p)
	p = strings.TrimRight(p, "/")

	// Ensure leading slash for file:// URL path
	if len(p) > 0 && p[0] != '/' {
		p = "/" + p
	}

	return "file://" + p
}

func normalizeFileURL(u *url.URL) string {
	p := u.Path
	if u.Host != "" {
		p = u.Host + p
	}
	p = strings.ReplaceAll(p, "\\", "/")

	// Resolve absolute and symlinks
	osPath := p
	if runtime.GOOS == "windows" && len(osPath) > 0 && osPath[0] == '/' {
		osPath = osPath[1:] // strip leading / for Windows
	}
	abs, err := filepath.Abs(strings.ReplaceAll(osPath, "/", string(filepath.Separator)))
	if err == nil {
		resolved, err2 := filepath.EvalSymlinks(abs)
		if err2 == nil {
			p = filepath.ToSlash(resolved)
		} else {
			p = filepath.ToSlash(abs)
		}
	}

	if runtime.GOOS == "windows" && len(p) >= 2 && p[1] == ':' {
		p = strings.ToLower(p[:1]) + p[1:]
	}

	p = collapseSlashes(p)
	p = strings.TrimRight(p, "/")

	if len(p) > 0 && p[0] != '/' {
		p = "/" + p
	}

	return "file://" + p
}

func normalizeSFTPURL(u *url.URL) string {
	host := strings.ToLower(u.Hostname())
	port := u.Port()
	if port == "22" {
		port = ""
	}

	path := u.Path
	path = collapseSlashes(path)
	path = strings.TrimRight(path, "/")
	path = percentDecodeUnreserved(path)

	result := "sftp://"
	if u.User != nil {
		result += u.User.Username() + "@"
	}
	result += host
	if port != "" {
		result += ":" + port
	}
	result += path

	return result
}

var multiSlash = regexp.MustCompile(`//+`)

func collapseSlashes(s string) string {
	return multiSlash.ReplaceAllString(s, "/")
}

func percentDecodeUnreserved(s string) string {
	// Decode percent-encoded unreserved characters: A-Z a-z 0-9 - . _ ~
	result := strings.Builder{}
	i := 0
	for i < len(s) {
		if i+2 < len(s) && s[i] == '%' {
			hi := unhex(s[i+1])
			lo := unhex(s[i+2])
			if hi >= 0 && lo >= 0 {
				c := byte(hi<<4 | lo)
				if isUnreserved(c) {
					result.WriteByte(c)
					i += 3
					continue
				}
			}
		}
		result.WriteByte(s[i])
		i++
	}
	return result.String()
}

func unhex(c byte) int {
	switch {
	case c >= '0' && c <= '9':
		return int(c - '0')
	case c >= 'a' && c <= 'f':
		return int(c - 'a' + 10)
	case c >= 'A' && c <= 'F':
		return int(c - 'A' + 10)
	}
	return -1
}

func isUnreserved(c byte) bool {
	return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') ||
		c == '-' || c == '.' || c == '_' || c == '~'
}

// OSPath converts a file:// URL path to an OS-native path.
// On Windows, strips the leading slash from /c:/path -> c:/path.
func OSPath(fileURL string) string {
	const prefix = "file://"
	if !strings.HasPrefix(fileURL, prefix) {
		return fileURL
	}
	p := fileURL[len(prefix):]
	if runtime.GOOS == "windows" && len(p) >= 3 && p[0] == '/' && isLetter(p[1]) && p[2] == ':' {
		p = p[1:]
	}
	return p
}

// Scheme returns the scheme of a normalized URL ("file" or "sftp").
func Scheme(normalizedURL string) string {
	if strings.HasPrefix(normalizedURL, "file://") {
		return "file"
	}
	if strings.HasPrefix(normalizedURL, "sftp://") {
		return "sftp"
	}
	return ""
}

// ParseSFTP extracts user, host, port, path from a normalized sftp:// URL.
func ParseSFTP(normalizedURL string) (user, host, port, path string) {
	const prefix = "sftp://"
	if !strings.HasPrefix(normalizedURL, prefix) {
		return
	}
	rest := normalizedURL[len(prefix):]

	// user@host:port/path
	if idx := strings.Index(rest, "@"); idx >= 0 {
		user = rest[:idx]
		rest = rest[idx+1:]
	}

	// Find path start
	pathIdx := strings.Index(rest, "/")
	hostport := rest
	if pathIdx >= 0 {
		hostport = rest[:pathIdx]
		path = rest[pathIdx:]
	}

	if cidx := strings.LastIndex(hostport, ":"); cidx >= 0 {
		host = hostport[:cidx]
		port = hostport[cidx+1:]
	} else {
		host = hostport
		port = "22"
	}

	return
}

// ParseQueryParams extracts mc and ct from a raw URL's query string.
func ParseQueryParams(rawURL string) (mc int, ct int, hasMC bool, hasCT bool) {
	u, err := url.Parse(rawURL)
	if err != nil {
		return
	}
	q := u.Query()
	if v := q.Get("mc"); v != "" {
		var n int
		if _, err := fmt.Sscanf(v, "%d", &n); err == nil && n > 0 {
			mc = n
			hasMC = true
		}
	}
	if v := q.Get("ct"); v != "" {
		var n int
		if _, err := fmt.Sscanf(v, "%d", &n); err == nil && n > 0 {
			ct = n
			hasCT = true
		}
	}
	return
}

