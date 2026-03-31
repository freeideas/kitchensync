package cli

import (
	"fmt"
	"strconv"
	"strings"

	"kitchensync/internal/log"
	"kitchensync/internal/urlutil"
)

// Options holds parsed global options.
type Options struct {
	MC int // max connections per URL
	CT int // connection timeout seconds
	VL log.Level
	XD int // stale TMP cleanup days
	BD int // BAK retention days
	TD int // tombstone retention days
}

// PeerRole indicates the role prefix on a peer.
type PeerRole int

const (
	RoleNormal      PeerRole = iota
	RoleCanon                // +
	RoleSubordinate          // -
)

// PeerArg represents a parsed peer argument.
type PeerArg struct {
	Role PeerRole
	URLs []*urlutil.NormalizedURL // fallback URLs in order
}

// DefaultOptions returns options with spec defaults.
func DefaultOptions() Options {
	return Options{MC: 10, CT: 30, VL: log.LevelInfo, XD: 2, BD: 90, TD: 180}
}

// Parse parses command-line arguments into options and peers.
// Returns (options, peers, helpRequested, error).
func Parse(args []string) (Options, []PeerArg, bool, error) {
	opts := DefaultOptions()
	var peers []PeerArg

	if len(args) == 0 {
		return opts, nil, true, nil
	}

	i := 0
	for i < len(args) {
		arg := args[i]

		// Help flags
		if arg == "-h" || arg == "--help" || arg == "/?" {
			return opts, nil, true, nil
		}

		// Options
		if arg == "--mc" || arg == "--ct" || arg == "-vl" || arg == "--xd" || arg == "--bd" || arg == "--td" {
			if i+1 >= len(args) {
				return opts, nil, false, fmt.Errorf("option %s requires a value", arg)
			}
			val := args[i+1]
			i += 2
			switch arg {
			case "--mc":
				n, err := strconv.Atoi(val)
				if err != nil || n < 1 {
					return opts, nil, false, fmt.Errorf("--mc must be a positive integer, got %q", val)
				}
				opts.MC = n
			case "--ct":
				n, err := strconv.Atoi(val)
				if err != nil || n < 1 {
					return opts, nil, false, fmt.Errorf("--ct must be a positive integer, got %q", val)
				}
				opts.CT = n
			case "-vl":
				level, ok := log.ParseLevel(val)
				if !ok {
					return opts, nil, false, fmt.Errorf("-vl must be one of: error, info, debug, trace; got %q", val)
				}
				opts.VL = level
			case "--xd":
				n, err := strconv.Atoi(val)
				if err != nil || n < 0 {
					return opts, nil, false, fmt.Errorf("--xd must be a non-negative integer, got %q", val)
				}
				opts.XD = n
			case "--bd":
				n, err := strconv.Atoi(val)
				if err != nil || n < 0 {
					return opts, nil, false, fmt.Errorf("--bd must be a non-negative integer, got %q", val)
				}
				opts.BD = n
			case "--td":
				n, err := strconv.Atoi(val)
				if err != nil || n < 0 {
					return opts, nil, false, fmt.Errorf("--td must be a non-negative integer, got %q", val)
				}
				opts.TD = n
			}
			continue
		}

		// Check for unrecognized flags
		if strings.HasPrefix(arg, "--") && !strings.HasPrefix(arg, "--mc") && !strings.HasPrefix(arg, "--ct") && !strings.HasPrefix(arg, "--xd") && !strings.HasPrefix(arg, "--bd") && !strings.HasPrefix(arg, "--td") {
			return opts, nil, false, fmt.Errorf("unrecognized option: %s", arg)
		}

		// Peer argument
		peer, err := parsePeerArg(arg)
		if err != nil {
			return opts, nil, false, err
		}
		peers = append(peers, peer)
		i++
	}

	// Validate: at most 1 canon
	canonCount := 0
	for _, p := range peers {
		if p.Role == RoleCanon {
			canonCount++
		}
	}
	if canonCount > 1 {
		return opts, nil, false, fmt.Errorf("at most one canon (+) peer allowed, got %d", canonCount)
	}

	return opts, peers, false, nil
}

func parsePeerArg(arg string) (PeerArg, error) {
	role := RoleNormal
	s := arg

	// Check prefix
	if strings.HasPrefix(s, "+") {
		role = RoleCanon
		s = s[1:]
	} else if strings.HasPrefix(s, "-") && !strings.HasPrefix(s, "--") {
		// Distinguish from options: - followed by a path or bracket
		if len(s) > 1 && (s[1] == '/' || s[1] == '[' || s[1] == '.' || (s[1] >= 'a' && s[1] <= 'z') || (s[1] >= 'A' && s[1] <= 'Z')) {
			role = RoleSubordinate
			s = s[1:]
		}
	}

	peer := PeerArg{Role: role}

	// Check for bracket syntax [url1,url2,...]
	if strings.HasPrefix(s, "[") && strings.HasSuffix(s, "]") {
		inner := s[1 : len(s)-1]
		parts := splitFallbackURLs(inner)
		for _, part := range parts {
			u, err := urlutil.Normalize(strings.TrimSpace(part))
			if err != nil {
				return PeerArg{}, fmt.Errorf("invalid URL %q: %w", part, err)
			}
			peer.URLs = append(peer.URLs, u)
		}
	} else {
		u, err := urlutil.Normalize(s)
		if err != nil {
			return PeerArg{}, fmt.Errorf("invalid URL %q: %w", s, err)
		}
		peer.URLs = append(peer.URLs, u)
	}

	return peer, nil
}

// splitFallbackURLs splits on commas but respects that SFTP URLs may contain colons.
func splitFallbackURLs(s string) []string {
	return strings.Split(s, ",")
}
