package logx

import (
	"fmt"
	"os"
	"time"
)

type Level int

const (
	LevelError Level = iota
	LevelWarn
	LevelInfo
	LevelDebug
	LevelTrace
)

var CurrentLevel = LevelInfo

func SetLevel(s string) error {
	switch s {
	case "error":
		CurrentLevel = LevelError
	case "warn":
		CurrentLevel = LevelWarn
	case "info":
		CurrentLevel = LevelInfo
	case "debug":
		CurrentLevel = LevelDebug
	case "trace":
		CurrentLevel = LevelTrace
	default:
		return fmt.Errorf("invalid verbosity level: %q", s)
	}
	return nil
}

func timestamp() string {
	now := time.Now().UTC()
	return now.Format("2006-01-02_15-04-05") + fmt.Sprintf("_%06dZ", now.Nanosecond()/1000)
}

func log(level Level, format string, args ...any) {
	if level > CurrentLevel {
		return
	}
	msg := fmt.Sprintf(format, args...)
	fmt.Fprintf(os.Stdout, "%s %s\n", timestamp(), msg)
}

func Error(format string, args ...any) { log(LevelError, format, args...) }
func Warn(format string, args ...any)  { log(LevelWarn, format, args...) }
func Info(format string, args ...any)  { log(LevelInfo, format, args...) }
func Debug(format string, args ...any) { log(LevelDebug, format, args...) }
func Trace(format string, args ...any) { log(LevelTrace, format, args...) }
