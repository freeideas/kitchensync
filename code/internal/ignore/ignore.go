package ignore

import (
	"strings"

	gitignore "github.com/sabhiram/go-gitignore"
)

type Rules struct {
	lines   []string
	matcher *gitignore.GitIgnore
}

// BuiltinExcludes are always excluded and cannot be overridden.
var builtinExcludes = []string{".kitchensync"}

// NewRules creates an empty rule set with the implicit .git/ pattern.
func NewRules() *Rules {
	// .git/ is implicitly excluded but can be negated by .syncignore
	lines := []string{".git/"}
	m := gitignore.CompileIgnoreLines(lines...)
	return &Rules{lines: lines, matcher: m}
}

// Merge creates a new Rules that includes parent rules plus new patterns from a .syncignore file.
func (r *Rules) Merge(content string) *Rules {
	rawLines := strings.Split(content, "\n")
	var cleaned []string
	for _, line := range rawLines {
		line = strings.TrimRight(line, "\r")
		cleaned = append(cleaned, line)
	}
	allLines := make([]string, len(r.lines)+len(cleaned))
	copy(allLines, r.lines)
	copy(allLines[len(r.lines):], cleaned)
	m := gitignore.CompileIgnoreLines(allLines...)
	return &Rules{lines: allLines, matcher: m}
}

// Matches returns true if the given name should be excluded.
// name is the entry basename; isDir indicates whether it's a directory.
func (r *Rules) Matches(name string, isDir bool) bool {
	// Built-in excludes cannot be overridden
	for _, excl := range builtinExcludes {
		if name == excl {
			return true
		}
	}

	pathToCheck := name
	if isDir {
		pathToCheck = name + "/"
	}
	return r.matcher.MatchesPath(pathToCheck)
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

	pathToCheck := relPath
	if isDir {
		pathToCheck = relPath + "/"
	}
	return r.matcher.MatchesPath(pathToCheck)
}
