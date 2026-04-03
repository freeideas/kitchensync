package ignore

import (
	"strings"

	gitignore "github.com/sabhiram/go-gitignore"
)

type Rules struct {
	matchers []*gitignore.GitIgnore
}

// BuiltinExcludes are always excluded and cannot be overridden.
var builtinExcludes = []string{".kitchensync"}

// NewRules creates an empty rule set with the implicit .git/ pattern.
func NewRules() *Rules {
	// .git/ is implicitly excluded but can be negated by .syncignore
	m := gitignore.CompileIgnoreLines(".git/")
	return &Rules{matchers: []*gitignore.GitIgnore{m}}
}

// Merge creates a new Rules that includes parent rules plus new patterns from a .syncignore file.
func (r *Rules) Merge(content string) *Rules {
	lines := strings.Split(content, "\n")
	var cleaned []string
	for _, line := range lines {
		line = strings.TrimRight(line, "\r")
		cleaned = append(cleaned, line)
	}
	m := gitignore.CompileIgnoreLines(cleaned...)
	newMatchers := make([]*gitignore.GitIgnore, len(r.matchers)+1)
	copy(newMatchers, r.matchers)
	newMatchers[len(r.matchers)] = m
	return &Rules{matchers: newMatchers}
}

// Matches returns true if the given name should be excluded.
// name is the entry basename; relPath is the path relative to the sync root.
func (r *Rules) Matches(name string, isDir bool) bool {
	// Built-in excludes cannot be overridden
	for _, excl := range builtinExcludes {
		if name == excl {
			return true
		}
	}

	// Check matchers in order (last match wins per gitignore semantics)
	// We check each matcher and use the last definitive result
	matched := false
	for _, m := range r.matchers {
		pathToCheck := name
		if isDir {
			pathToCheck = name + "/"
		}
		if m.MatchesPath(pathToCheck) {
			matched = true
		}
	}
	return matched
}

// MatchesPath checks a relative path against the rules.
func (r *Rules) MatchesPath(relPath string, isDir bool) bool {
	// Built-in excludes
	parts := strings.Split(relPath, "/")
	for _, part := range parts {
		for _, excl := range builtinExcludes {
			if part == excl {
				return true
			}
		}
	}

	matched := false
	for _, m := range r.matchers {
		pathToCheck := relPath
		if isDir {
			pathToCheck = relPath + "/"
		}
		if m.MatchesPath(pathToCheck) {
			matched = true
		}
	}
	return matched
}
