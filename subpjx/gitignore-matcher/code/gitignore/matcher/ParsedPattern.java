package gitignore.matcher;

import java.util.regex.Pattern;

record ParsedPattern(boolean negated, boolean directoryOnly, Pattern regex) {}
