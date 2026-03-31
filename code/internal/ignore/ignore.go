package ignore

import (
	"strings"

	gitignore "github.com/sabhiram/go-gitignore"
)

// Rules wraps accumulated ignore patterns.
type Rules struct {
	lines   []string
	matcher *gitignore.GitIgnore
}

// NewRules creates an empty rules set.
func NewRules() *Rules {
	return &Rules{}
}

// DefaultRules creates a rules set with built-in default excludes.
// These can be overridden by .syncignore negation patterns (e.g. !.git/).
func DefaultRules() *Rules {
	lines := []string{".git/"}
	return &Rules{
		lines:   lines,
		matcher: gitignore.CompileIgnoreLines(lines...),
	}
}

// Merge returns a new Rules that combines parent rules with new patterns.
func (r *Rules) Merge(content string) *Rules {
	var newLines []string
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		newLines = append(newLines, line)
	}
	if len(newLines) == 0 {
		return r
	}

	combined := make([]string, len(r.lines), len(r.lines)+len(newLines))
	copy(combined, r.lines)
	combined = append(combined, newLines...)

	return &Rules{
		lines:   combined,
		matcher: gitignore.CompileIgnoreLines(combined...),
	}
}

// Matches returns true if the name should be ignored (for files).
func (r *Rules) Matches(name string) bool {
	if r == nil || r.matcher == nil {
		return false
	}
	return r.matcher.MatchesPath(name)
}

// MatchesDir returns true if the directory name should be ignored.
// It appends a trailing slash so directory-only patterns (e.g. build/) match.
func (r *Rules) MatchesDir(name string) bool {
	if r == nil || r.matcher == nil {
		return false
	}
	return r.matcher.MatchesPath(name + "/")
}

// IsBuiltinExclude checks built-in excludes that cannot be overridden.
func IsBuiltinExclude(name string) bool {
	return name == ".kitchensync"
}
