package snapshot.db;

import java.nio.charset.StandardCharsets;

public final class PathIdentity {

    private static final char[] ALPHABET =
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".toCharArray();

    private static final long P1 = 0x9E3779B185EBCA87L;
    private static final long P2 = 0xC2B2AE3D27D4EB4FL;
    private static final long P3 = 0x165667B19E3779F9L;
    private static final long P4 = 0x85EBCA77C2B2AE63L;
    private static final long P5 = 0x27D4EB2F165667C5L;

    private PathIdentity() {}

    public static String identify(String relativePath) {
        String input = (relativePath == null || relativePath.equals("/")) ? "" : relativePath;
        byte[] bytes = input.getBytes(StandardCharsets.UTF_8);
        long hash = xxHash64(bytes);
        return base62Encode(hash);
    }

    private static long xxHash64(byte[] data) {
        int len = data.length;
        int pos = 0;
        long h64;

        if (len >= 32) {
            long v1 = P1 + P2;
            long v2 = P2;
            long v3 = 0L;
            long v4 = -P1;

            int limit = len - 32;
            do {
                v1 = Long.rotateLeft(v1 + readLongLE(data, pos) * P2, 31) * P1; pos += 8;
                v2 = Long.rotateLeft(v2 + readLongLE(data, pos) * P2, 31) * P1; pos += 8;
                v3 = Long.rotateLeft(v3 + readLongLE(data, pos) * P2, 31) * P1; pos += 8;
                v4 = Long.rotateLeft(v4 + readLongLE(data, pos) * P2, 31) * P1; pos += 8;
            } while (pos <= limit);

            h64 = Long.rotateLeft(v1, 1) + Long.rotateLeft(v2, 7)
                + Long.rotateLeft(v3, 12) + Long.rotateLeft(v4, 18);
            h64 = mergeRound(h64, v1);
            h64 = mergeRound(h64, v2);
            h64 = mergeRound(h64, v3);
            h64 = mergeRound(h64, v4);
        } else {
            h64 = P5;
        }

        h64 += len;

        while (pos + 8 <= len) {
            h64 ^= Long.rotateLeft(readLongLE(data, pos) * P2, 31) * P1;
            h64 = Long.rotateLeft(h64, 27) * P1 + P4;
            pos += 8;
        }
        if (pos + 4 <= len) {
            h64 ^= (readIntLE(data, pos) & 0xFFFFFFFFL) * P1;
            h64 = Long.rotateLeft(h64, 23) * P2 + P3;
            pos += 4;
        }
        while (pos < len) {
            h64 ^= (data[pos] & 0xFFL) * P5;
            h64 = Long.rotateLeft(h64, 11) * P1;
            pos++;
        }

        h64 ^= h64 >>> 33;
        h64 *= P2;
        h64 ^= h64 >>> 29;
        h64 *= P3;
        h64 ^= h64 >>> 32;
        return h64;
    }

    private static long mergeRound(long acc, long val) {
        acc ^= Long.rotateLeft(val * P2, 31) * P1;
        return acc * P1 + P4;
    }

    private static long readLongLE(byte[] d, int p) {
        return (d[p] & 0xFFL)
             | ((d[p+1] & 0xFFL) <<  8)
             | ((d[p+2] & 0xFFL) << 16)
             | ((d[p+3] & 0xFFL) << 24)
             | ((d[p+4] & 0xFFL) << 32)
             | ((d[p+5] & 0xFFL) << 40)
             | ((d[p+6] & 0xFFL) << 48)
             | ((d[p+7] & 0xFFL) << 56);
    }

    private static int readIntLE(byte[] d, int p) {
        return (d[p] & 0xFF)
             | ((d[p+1] & 0xFF) <<  8)
             | ((d[p+2] & 0xFF) << 16)
             | ((d[p+3] & 0xFF) << 24);
    }

    private static String base62Encode(long value) {
        char[] result = new char[11];
        for (int i = 10; i >= 0; i--) {
            result[i] = ALPHABET[(int) Long.remainderUnsigned(value, 62)];
            value = Long.divideUnsigned(value, 62);
        }
        return new String(result);
    }
}
