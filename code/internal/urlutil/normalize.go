package urlutil

import (
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
)

// NormalizedURL represents a parsed, normalized peer URL.
type NormalizedURL struct {
	Scheme   string // "file" or "sftp"
	User     string
	Password string
	Host     string // hostname only (no port)
	Port     int    // 0 means default
	Path     string // absolute path, no trailing slash
	Raw      string // original before normalization
	// Per-URL settings
	MaxConn     int // 0 means use global default
	ConnTimeout int // 0 means use global default
}

// String returns the normalized URL string (without query params).
func (u *NormalizedURL) String() string {
	if u.Scheme == "file" {
		return "file://" + u.Path
	}
	s := "sftp://"
	if u.User != "" {
		s += u.User
		if u.Password != "" {
			s += ":" + url.PathEscape(u.Password)
		}
		s += "@"
	}
	s += u.Host
	if u.Port != 0 && u.Port != 22 {
		s += fmt.Sprintf(":%d", u.Port)
	}
	s += u.Path
	return s
}

// Identity returns the normalized URL used as pool key (no query params, no password).
func (u *NormalizedURL) Identity() string {
	if u.Scheme == "file" {
		return "file://" + u.Path
	}
	s := "sftp://"
	if u.User != "" {
		s += u.User + "@"
	}
	s += u.Host
	if u.Port != 0 && u.Port != 22 {
		s += fmt.Sprintf(":%d", u.Port)
	}
	s += u.Path
	return s
}

// OSPath returns the path in OS-native format for filesystem operations.
// On Windows, strips the leading slash from drive letter paths (e.g., /c:/foo -> c:/foo).
func (u *NormalizedURL) OSPath() string {
	p := u.Path
	// On Windows, /c:/... needs the leading slash stripped for OS calls
	if runtime.GOOS == "windows" && len(p) >= 3 && p[0] == '/' && p[2] == ':' {
		p = p[1:]
	}
	return p
}

var multiSlash = regexp.MustCompile(`/{2,}`)

// Normalize parses and normalizes a URL string.
func Normalize(raw string) (*NormalizedURL, error) {
	result := &NormalizedURL{Raw: raw}

	s := raw

	// Check for sftp:// scheme
	if strings.HasPrefix(strings.ToLower(s), "sftp://") {
		return parseSFTP(s, result)
	}

	// Check for file:// scheme
	if strings.HasPrefix(strings.ToLower(s), "file://") {
		s = s[7:]
		return parseLocal(s, result)
	}

	// Bare path
	return parseLocal(s, result)
}

func parseLocal(path string, result *NormalizedURL) (*NormalizedURL, error) {
	result.Scheme = "file"

	// Strip query string
	if idx := strings.Index(path, "?"); idx >= 0 {
		queryStr := path[idx+1:]
		path = path[:idx]
		if queryStr != "" {
			params, err := url.ParseQuery(queryStr)
			if err == nil {
				if v := params.Get("mc"); v != "" {
					fmt.Sscanf(v, "%d", &result.MaxConn)
				}
				if v := params.Get("ct"); v != "" {
					fmt.Sscanf(v, "%d", &result.ConnTimeout)
				}
			}
		}
	}

	// Resolve to absolute
	p := filepath.Clean(path)
	if !filepath.IsAbs(p) {
		cwd, err := os.Getwd()
		if err != nil {
			return nil, fmt.Errorf("cannot resolve relative path: %w", err)
		}
		p = filepath.Join(cwd, p)
	}

	// Convert to forward slashes
	p = filepath.ToSlash(p)

	// On Windows, ensure drive letter paths have leading slash: /c:/...
	if runtime.GOOS == "windows" && len(p) >= 2 && p[1] == ':' {
		p = "/" + p
	}

	// Remove trailing slash
	p = strings.TrimRight(p, "/")
	if p == "" {
		p = "/"
	}

	// Collapse multiple slashes
	p = multiSlash.ReplaceAllString(p, "/")

	// Percent-decode unreserved characters in path
	if decoded, err := url.PathUnescape(p); err == nil {
		p = decoded
	}

	result.Path = p
	return result, nil
}

func parseSFTP(raw string, result *NormalizedURL) (*NormalizedURL, error) {
	result.Scheme = "sftp"

	// Separate query string first
	queryStr := ""
	mainPart := raw
	if idx := strings.Index(raw, "?"); idx >= 0 {
		queryStr = raw[idx+1:]
		mainPart = raw[:idx]
	}

	// Parse query params
	if queryStr != "" {
		params, err := url.ParseQuery(queryStr)
		if err == nil {
			if v := params.Get("mc"); v != "" {
				fmt.Sscanf(v, "%d", &result.MaxConn)
			}
			if v := params.Get("ct"); v != "" {
				fmt.Sscanf(v, "%d", &result.ConnTimeout)
			}
		}
	}

	// Strip scheme
	s := mainPart[7:] // after "sftp://"
	if strings.HasPrefix(strings.ToLower(mainPart), "sftp://") {
		s = mainPart[7:]
	} else {
		s = mainPart[7:]
	}

	// Extract userinfo@host:port/path
	// Find first / after host portion
	slashIdx := strings.Index(s, "/")
	hostPart := s
	pathPart := "/"
	if slashIdx >= 0 {
		hostPart = s[:slashIdx]
		pathPart = s[slashIdx:]
	}

	// Parse user:pass@host:port
	if atIdx := strings.LastIndex(hostPart, "@"); atIdx >= 0 {
		userInfo := hostPart[:atIdx]
		hostPart = hostPart[atIdx+1:]
		if colonIdx := strings.Index(userInfo, ":"); colonIdx >= 0 {
			result.User = userInfo[:colonIdx]
			decoded, err := url.PathUnescape(userInfo[colonIdx+1:])
			if err != nil {
				result.Password = userInfo[colonIdx+1:]
			} else {
				result.Password = decoded
			}
		} else {
			result.User = userInfo
		}
	}

	// Parse host:port
	result.Host = strings.ToLower(hostPart)
	if colonIdx := strings.LastIndex(result.Host, ":"); colonIdx >= 0 {
		portStr := result.Host[colonIdx+1:]
		result.Host = result.Host[:colonIdx]
		fmt.Sscanf(portStr, "%d", &result.Port)
		if result.Port == 22 {
			result.Port = 0 // strip default port
		}
	}

	// Normalize path
	pathPart = multiSlash.ReplaceAllString(pathPart, "/")
	pathPart = strings.TrimRight(pathPart, "/")
	if pathPart == "" {
		pathPart = "/"
	}

	// Percent-decode unreserved characters in path
	if decoded, err := url.PathUnescape(pathPart); err == nil {
		pathPart = decoded
	}

	result.Path = pathPart
	return result, nil
}
