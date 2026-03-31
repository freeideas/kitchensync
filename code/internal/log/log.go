package log

import (
	"fmt"
	"os"
	"strings"
)

type Level int

const (
	LevelError Level = iota
	LevelInfo
	LevelDebug
	LevelTrace
)

var currentLevel = LevelInfo

func SetLevel(l Level) { currentLevel = l }
func GetLevel() Level  { return currentLevel }

func ParseLevel(s string) (Level, bool) {
	switch strings.ToLower(s) {
	case "error":
		return LevelError, true
	case "info":
		return LevelInfo, true
	case "debug":
		return LevelDebug, true
	case "trace":
		return LevelTrace, true
	}
	return 0, false
}

func log(level Level, prefix, format string, args ...any) {
	if level > currentLevel {
		return
	}
	msg := fmt.Sprintf(format, args...)
	fmt.Fprintf(os.Stdout, "%s %s\n", prefix, msg)
}

func Error(format string, args ...any) { log(LevelError, "ERROR", format, args...) }
func Info(format string, args ...any)  { log(LevelInfo, "INFO", format, args...) }
func Debug(format string, args ...any) { log(LevelDebug, "DEBUG", format, args...) }
func Trace(format string, args ...any) { log(LevelTrace, "TRACE", format, args...) }
