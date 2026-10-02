#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Repair exFAT names that macOS cannot open, by rewriting them in NFC on disk.

An accented letter can be stored as one character (NFC, "é" = U+00E9) or as a
letter plus a combining accent (NFD, "e" + U+0301). Linux's exFAT driver stores
whatever bytes it is given, so an NFD name can land on an exFAT drive (for
example, written by a KitchenSync run from Linux before names were created in
NFC). macOS's exFAT driver lists such a name but cannot open, rename or delete
it: it looks names up in NFC, and the name hash stored on disk is for the NFD
form. KitchenSync then reports "listing failed ... No such file or directory"
for it on every run. See specs/sync.md, "Unicode Normalization".

This script reads the raw exFAT volume, finds every name under a directory that
is not in NFC, and rewrites it in NFC in place: the name characters, the name
length, the name hash and the entry-set checksum. File contents are untouched.
A name is skipped (and reported) if its NFC form already exists in the same
directory or would need a different number of directory entries.

Usage (the volume must be UNMOUNTED for --write; without --write nothing is
changed and the planned renames are listed):

    sudo uv run --script extart/exfat-fix-nfd-names.py /dev/rdisk4s2 acex/movx
    diskutil unmount /Volumes/Movx
    sudo uv run --script extart/exfat-fix-nfd-names.py /dev/rdisk4s2 acex/movx --write
    sudo fsck_exfat -n /dev/rdisk4s2
    diskutil mount disk4s2

The path is relative to the volume root (use "" for the whole volume). On Linux
the device is something like /dev/sdb1. With --write, the original bytes of
every changed entry are appended to exfat-fix-backup.txt in the current
directory as "<device offset> <hex>" lines.
"""

import argparse
import struct
import subprocess
import sys
import unicodedata

BLK = 4096


class Volume:
    def __init__(self, dev: str, write: bool):
        self.f = open(dev, "r+b" if write else "rb")
        bs = self.read(0, 512)
        if bs[3:11] != b"EXFAT   ":
            sys.exit(f"{dev} is not an exFAT volume")
        self.fat_off = struct.unpack_from("<I", bs, 80)[0]
        self.heap_off = struct.unpack_from("<I", bs, 88)[0]
        self.root = struct.unpack_from("<I", bs, 96)[0]
        self.sec = 1 << bs[108]
        self.csize = self.sec << bs[109]

    # Raw devices need block-aligned reads and writes.
    def read(self, off: int, n: int) -> bytes:
        a = off - off % BLK
        self.f.seek(a)
        return self.f.read(((off + n - a + BLK - 1) // BLK) * BLK)[off - a: off - a + n]

    def write32(self, off: int, data: bytes):
        a = off - off % BLK
        self.f.seek(a)
        block = bytearray(self.f.read(BLK))
        block[off - a: off - a + 32] = data
        self.f.seek(a)
        self.f.write(block)

    def chain(self, c: int, nofat: bool, length: int) -> list[int]:
        if nofat:
            return list(range(c, c + max(1, (length + self.csize - 1) // self.csize)))
        out = []
        while 2 <= c < 0xFFFFFFF7:
            out.append(c)
            c = struct.unpack("<I", self.read(self.fat_off * self.sec + c * 4, 4))[0]
        return out

    def entries(self, c: int, nofat: bool = False, length: int = 0) -> list[dict]:
        """The file entry sets of one directory, with each 32-byte slot's offset."""
        offs, data = [], b""
        for x in self.chain(c, nofat, length):
            base = (self.heap_off * self.sec) + (x - 2) * self.csize
            data += self.read(base, self.csize)
            offs += range(base, base + self.csize, 32)
        out, i = [], 0
        while i < len(data) and data[i] != 0:
            if data[i] == 0x85:
                sc = data[i + 1]
                raw = data[i: i + 32 * (sc + 1)]
                st = raw[32:64]
                nlen = st[3]
                name = b"".join(raw[64 + 32 * k + 2: 64 + 32 * k + 32] for k in range(sc - 1))[: nlen * 2]
                out.append(dict(
                    name=name.decode("utf-16le", "replace"), sc=sc, raw=raw, slots=offs[i // 32: i // 32 + sc + 1],
                    is_dir=bool(struct.unpack_from("<H", raw, 4)[0] & 0x10),
                    first=struct.unpack_from("<I", st, 20)[0], dlen=struct.unpack_from("<Q", st, 24)[0], nofat=bool(st[1] & 2)))
                i += 32 * (sc + 1)
            else:
                i += 32
        return out


def checksum(raw: bytes) -> int:
    s = 0
    for k, b in enumerate(raw):
        if k not in (2, 3):
            s = (((s >> 1) | ((s & 1) << 15)) + b) & 0xFFFF
    return s


def name_hash(name: str) -> int:
    # exFAT hashes the up-cased name; Python's upper() matches the standard
    # up-case table for the letters that occur in practice.
    s = 0
    for ch in name.upper():
        for b in ch.encode("utf-16le"):
            s = (((s >> 1) | ((s & 1) << 15)) + b) & 0xFFFF
    return s


def renamed(e: dict, new: str) -> bytes:
    raw = bytearray(e["raw"])
    raw[32 + 3] = len(new)
    struct.pack_into("<H", raw, 32 + 4, name_hash(new))
    units = new.encode("utf-16le").ljust(30 * (e["sc"] - 1), b"\0")
    for k in range(e["sc"] - 1):
        raw[64 + 32 * k + 2: 64 + 32 * k + 32] = units[30 * k: 30 * k + 30]
    struct.pack_into("<H", raw, 2, checksum(raw))
    return bytes(raw)


def mounted(dev: str) -> bool:
    plain = dev.replace("/dev/rdisk", "/dev/disk")
    try:
        out = subprocess.run(["mount"], capture_output=True, text=True).stdout
    except OSError:
        return False
    return any(line.split(" ", 1)[0] in (dev, plain) for line in out.splitlines())


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("device")
    ap.add_argument("path", help="directory relative to the volume root")
    ap.add_argument("--write", action="store_true", help="apply the renames (volume must be unmounted)")
    args = ap.parse_args()
    if args.write and mounted(args.device):
        sys.exit(f"{args.device} is mounted; unmount it before --write")

    vol = Volume(args.device, args.write)
    cur = vol.entries(vol.root)
    for comp in [p for p in args.path.split("/") if p]:
        e = next((e for e in cur if unicodedata.normalize("NFC", e["name"]) == unicodedata.normalize("NFC", comp)), None)
        if e is None or not e["is_dir"]:
            sys.exit(f"no directory {comp!r} on the volume")
        cur = vol.entries(e["first"], e["nofat"], e["dlen"])

    fixed = skipped = 0
    backup = open("exfat-fix-backup.txt", "a") if args.write else None
    stack = [(args.path.strip("/"), cur)]
    while stack:
        where, ents = stack.pop()
        names = {e["name"] for e in ents}
        for e in ents:
            rel = f"{where}/{e['name']}" if where else e["name"]
            new = unicodedata.normalize("NFC", e["name"])
            if new != e["name"]:
                if new in names:
                    print(f"skip (NFC name already present): {rel}")
                    skipped += 1
                elif 1 + (len(new) + 14) // 15 != e["sc"]:
                    print(f"skip (entry count would change): {rel}")
                    skipped += 1
                else:
                    print(f"{'fix' if args.write else 'would fix'}: {rel}")
                    if args.write:
                        raw = renamed(e, new)
                        for o, k in zip(e["slots"], range(0, len(raw), 32)):
                            backup.write(f"{o} {e['raw'][k:k + 32].hex()}\n")
                            vol.write32(o, raw[k:k + 32])
                    fixed += 1
            if e["is_dir"]:
                stack.append((rel, vol.entries(e["first"], e["nofat"], e["dlen"])))
    if backup:
        backup.close()
        vol.f.flush()
    print(f"{'fixed' if args.write else 'would fix'} {fixed}, skipped {skipped}")


if __name__ == "__main__":
    main()
