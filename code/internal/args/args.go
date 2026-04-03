package args

import (
	"fmt"
	"kitchensync/internal/help"
	"kitchensync/internal/urlnorm"
	"strconv"
	"strings"
)

type Options struct {
	DryRun  bool
	Watch   bool
	MC      int
	CT      int
	VL      string
	XD      int
	BD      int
	TD      int
	SI      int
}

type PeerURL struct {
	Raw        string
	Normalized string
	MC         int // 0 means use global
	CT         int // 0 means use global
}

type Peer struct {
	URLs         []PeerURL
	IsCanon      bool
	IsSubordinate bool
}

type Config struct {
	Options Options
	Peers   []*Peer
}

func DefaultOptions() Options {
	return Options{
		MC: 10,
		CT: 30,
		VL: "info",
		XD: 2,
		BD: 90,
		TD: 180,
		SI: 30,
	}
}

func Parse(argv []string) (*Config, error) {
	if len(argv) == 0 {
		return nil, &HelpRequested{}
	}

	opts := DefaultOptions()
	var peerArgs []string

	i := 0
	for i < len(argv) {
		arg := argv[i]
		switch arg {
		case "-h", "--help", "/?":
			return nil, &HelpRequested{}
		case "-n", "--dry-run":
			opts.DryRun = true
		case "--watch":
			opts.Watch = true
		case "--mc":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "--mc requires a value"}
			}
			v, err := strconv.Atoi(argv[i])
			if err != nil || v < 1 {
				return nil, &ValidationError{Msg: "--mc must be a positive integer"}
			}
			opts.MC = v
		case "--ct":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "--ct requires a value"}
			}
			v, err := strconv.Atoi(argv[i])
			if err != nil || v < 1 {
				return nil, &ValidationError{Msg: "--ct must be a positive integer"}
			}
			opts.CT = v
		case "-vl":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "-vl requires a value"}
			}
			switch argv[i] {
			case "error", "warn", "info", "debug", "trace":
				opts.VL = argv[i]
			default:
				return nil, &ValidationError{Msg: fmt.Sprintf("invalid verbosity level: %q (must be error, warn, info, debug, trace)", argv[i])}
			}
		case "--xd":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "--xd requires a value"}
			}
			v, err := strconv.Atoi(argv[i])
			if err != nil || v < 0 {
				return nil, &ValidationError{Msg: "--xd must be a non-negative integer"}
			}
			opts.XD = v
		case "--bd":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "--bd requires a value"}
			}
			v, err := strconv.Atoi(argv[i])
			if err != nil || v < 0 {
				return nil, &ValidationError{Msg: "--bd must be a non-negative integer"}
			}
			opts.BD = v
		case "--td":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "--td requires a value"}
			}
			v, err := strconv.Atoi(argv[i])
			if err != nil || v < 0 {
				return nil, &ValidationError{Msg: "--td must be a non-negative integer"}
			}
			opts.TD = v
		case "--si":
			i++
			if i >= len(argv) {
				return nil, &ValidationError{Msg: "--si requires a value"}
			}
			v, err := strconv.Atoi(argv[i])
			if err != nil || v < 1 {
				return nil, &ValidationError{Msg: "--si must be a positive integer"}
			}
			opts.SI = v
		default:
			if strings.HasPrefix(arg, "--") || (strings.HasPrefix(arg, "-") && len(arg) > 1 && arg != "-" && !isPathStart(arg)) {
				// Check if it looks like an unknown flag vs a path starting with -
				if strings.HasPrefix(arg, "--") {
					return nil, &ValidationError{Msg: fmt.Sprintf("unrecognized option: %s", arg)}
				}
				// -something could be a subordinate peer
				peerArgs = append(peerArgs, arg)
			} else {
				peerArgs = append(peerArgs, arg)
			}
		}
		i++
	}

	if len(peerArgs) == 0 {
		return nil, &ValidationError{Msg: "no peers specified"}
	}

	peers, err := parsePeers(peerArgs)
	if err != nil {
		return nil, err
	}

	// Validate: at most 1 canon
	canonCount := 0
	for _, p := range peers {
		if p.IsCanon {
			canonCount++
		}
	}
	if canonCount > 1 {
		return nil, &ValidationError{Msg: "only one canon (+) peer is allowed"}
	}

	return &Config{Options: opts, Peers: peers}, nil
}

func isPathStart(s string) bool {
	// A subordinate peer starts with - followed by a path character
	if len(s) < 2 {
		return false
	}
	c := s[1]
	return c == '/' || c == '.' || c == '[' || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')
}

func parsePeers(peerArgs []string) ([]*Peer, error) {
	var peers []*Peer
	for _, arg := range peerArgs {
		peer, err := parsePeer(arg)
		if err != nil {
			return nil, err
		}
		peers = append(peers, peer)
	}
	return peers, nil
}

func parsePeer(arg string) (*Peer, error) {
	isCanon := false
	isSub := false

	raw := arg
	if strings.HasPrefix(raw, "+") {
		isCanon = true
		raw = raw[1:]
	} else if strings.HasPrefix(raw, "-") && len(raw) > 1 && raw[1] != '-' {
		isSub = true
		raw = raw[1:]
	}

	var urls []PeerURL

	if strings.HasPrefix(raw, "[") {
		// Fallback URL list
		if !strings.HasSuffix(raw, "]") {
			return nil, &ValidationError{Msg: fmt.Sprintf("malformed fallback URL list: %s", arg)}
		}
		inner := raw[1 : len(raw)-1]
		parts := strings.Split(inner, ",")
		for _, p := range parts {
			p = strings.TrimSpace(p)
			if p == "" {
				continue
			}
			pu := parsePeerURL(p)
			urls = append(urls, pu)
		}
	} else {
		pu := parsePeerURL(raw)
		urls = append(urls, pu)
	}

	if len(urls) == 0 {
		return nil, &ValidationError{Msg: fmt.Sprintf("empty peer URL: %s", arg)}
	}

	return &Peer{
		URLs:          urls,
		IsCanon:       isCanon,
		IsSubordinate: isSub,
	}, nil
}

func parsePeerURL(raw string) PeerURL {
	mc, ct, _, _ := urlnorm.ParseQueryParams(raw)
	norm := urlnorm.Normalize(raw)
	return PeerURL{
		Raw:        raw,
		Normalized: norm,
		MC:         mc,
		CT:         ct,
	}
}

type HelpRequested struct{}

func (e *HelpRequested) Error() string {
	return help.Text
}

type ValidationError struct {
	Msg string
}

func (e *ValidationError) Error() string {
	return e.Msg + "\n\n" + help.Text
}
