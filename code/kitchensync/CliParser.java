package kitchensync;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

final class CliParser {
    private CliParser() {
    }

    static Parsed parse(String[] args) {
        if (args.length == 0 || Arrays.asList(args).contains("-h") || Arrays.asList(args).contains("--help")
                || Arrays.asList(args).contains("/?")) {
            return Parsed.help();
        }
        RunOptions options = new RunOptions();
        int canonCount = 0;
        for (int i = 0; i < args.length; i++) {
            String arg = args[i];
            switch (arg) {
                case "--mc" -> options.maxConnections = positiveInt(value(args, ++i, arg), arg);
                case "--ct" -> options.connectTimeoutSeconds = positiveInt(value(args, ++i, arg), arg);
                case "--ka" -> options.keepAliveSeconds = positiveInt(value(args, ++i, arg), arg);
                case "--dir-status" -> options.dirStatusSeconds = nonNegativeInt(value(args, ++i, arg), arg);
                case "--xd" -> options.tmpRetentionDays = positiveInt(value(args, ++i, arg), arg);
                case "--bd" -> options.bakRetentionDays = positiveInt(value(args, ++i, arg), arg);
                case "--td" -> options.tombstoneRetentionDays = positiveInt(value(args, ++i, arg), arg);
                case "-vl" -> options.verbosity = verbosity(value(args, ++i, arg));
                default -> {
                    if (arg.startsWith("-") && !arg.startsWith("-/") && !arg.startsWith("-[")
                            && !looksLikeUrl(arg.substring(1)) && !looksLikeWindowsPath(arg.substring(1))) {
                        throw new ValidationException("Unrecognized flag: " + arg);
                    }
                    PeerArgument peer = peer(arg, options.peers.size());
                    if (peer.modifier() == PeerModifier.CANON) {
                        canonCount++;
                    }
                    options.peers.add(peer);
                }
            }
        }
        if (options.peers.size() < 2) {
            throw new ValidationException("At least two peers are required");
        }
        if (canonCount > 1) {
            throw new ValidationException("At most one canon peer is allowed");
        }
        return Parsed.run(options);
    }

    private static String value(String[] args, int index, String flag) {
        if (index >= args.length) {
            throw new ValidationException("Missing value for " + flag);
        }
        return args[index];
    }

    private static int positiveInt(String text, String flag) {
        try {
            int value = Integer.parseInt(text);
            if (value <= 0) {
                throw new NumberFormatException();
            }
            return value;
        } catch (NumberFormatException ex) {
            throw new ValidationException("Invalid value for " + flag + ": " + text);
        }
    }

    private static int nonNegativeInt(String text, String flag) {
        try {
            int value = Integer.parseInt(text);
            if (value < 0) {
                throw new NumberFormatException();
            }
            return value;
        } catch (NumberFormatException ex) {
            throw new ValidationException("Invalid value for " + flag + ": " + text);
        }
    }

    private static Verbosity verbosity(String text) {
        try {
            return Verbosity.valueOf(text);
        } catch (IllegalArgumentException ex) {
            throw new ValidationException("Invalid value for -vl: " + text);
        }
    }

    private static PeerArgument peer(String raw, int index) {
        PeerModifier modifier = PeerModifier.NORMAL;
        if (raw.startsWith("+")) {
            modifier = PeerModifier.CANON;
            raw = raw.substring(1);
        } else if (raw.startsWith("-")) {
            modifier = PeerModifier.SUBORDINATE;
            raw = raw.substring(1);
        }
        List<String> urls;
        if (raw.startsWith("[") && raw.endsWith("]")) {
            String inner = raw.substring(1, raw.length() - 1);
            urls = new ArrayList<>();
            for (String part : inner.split(",")) {
                if (!part.isBlank()) {
                    urls.add(part.trim());
                }
            }
        } else {
            urls = List.of(raw);
        }
        if (urls.isEmpty()) {
            throw new ValidationException("Peer has no URL");
        }
        return new PeerArgument(modifier, urls, index);
    }

    private static boolean looksLikeWindowsPath(String text) {
        return text.length() >= 2 && Character.isLetter(text.charAt(0)) && text.charAt(1) == ':';
    }

    private static boolean looksLikeUrl(String text) {
        return text.regionMatches(true, 0, "sftp://", 0, "sftp://".length())
                || text.regionMatches(true, 0, "file://", 0, "file://".length());
    }

    record Parsed(boolean isHelp, RunOptions options) {
        static Parsed help() {
            return new Parsed(true, null);
        }

        static Parsed run(RunOptions options) {
            return new Parsed(false, options);
        }
    }

    static final class ValidationException extends RuntimeException {
        ValidationException(String message) {
            super(message);
        }
    }
}
