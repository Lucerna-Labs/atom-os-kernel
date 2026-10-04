#!/usr/bin/env python3
"""Create an Atom OS data disk (ATOMFS02) holding files from the host.

    python3 scripts/atomfs-pack.py disk.img --size 64M --add hello=/linux/hello --add notes.txt=/docs/notes.txt

Each --add copies a host file to a path on the disk (folders are created). The
result is one saved generation in the copy-on-write format kernel-kit/src/storage.rs
reads: 4 KiB blocks, two superblock slots (generation 1 goes in slot 1), checksummed
metadata listing every folder and file with its extents, and file contents checked
against their FNV-1a checksum when first opened. Boot it as the data disk
(scripts/boot-test.py's Guest, scripts/run.py --disk) and the files are there.
"""
import argparse
from pathlib import Path
import struct

BLOCK = 4096
MAGIC = b"ATOMFS02"
VERSION = 2


def checksum(data):
    value = 0xcbf29ce484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return value


def size_arg(text):
    units = {"K": 1 << 10, "M": 1 << 20, "G": 1 << 30}
    return int(text[:-1]) * units[text[-1].upper()] if text[-1].upper() in units else int(text)


def valid_name(name):
    return 0 < len(name.encode()) <= 255 and name not in (".", "..") and all(31 < ord(c) != 127 and c != "/" for c in name)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("image", type=Path)
    parser.add_argument("--size", type=size_arg, default=64 << 20, help="disk size (default 64M)")
    parser.add_argument("--add", action="append", default=[], metavar="HOST=DISK", help="copy HOST file to DISK path")
    args = parser.parse_args()
    if args.size % BLOCK or args.size < 64 * BLOCK:
        parser.error("the size must be a multiple of 4 KiB and at least 256 KiB")
    total = args.size // BLOCK

    # Folder tree: path tuple -> id, files as (id, parent, name, bytes).
    folders = {(): 0}
    entries = []  # (id, parent, is_dir, name, data)
    next_id = 1
    for spec in args.add:
        host, _, disk = spec.partition("=")
        parts = [p for p in disk.split("/") if p]
        if not host or not parts or not all(valid_name(p) for p in parts):
            parser.error(f"bad --add {spec!r}")
        for depth in range(1, len(parts)):
            key = tuple(parts[:depth])
            if key not in folders:
                folders[key] = next_id
                entries.append((next_id, folders[key[:-1]], True, parts[depth - 1], b""))
                next_id += 1
        if tuple(parts) in folders or any(e[1] == folders[tuple(parts[:-1])] and e[3] == parts[-1] for e in entries):
            parser.error(f"{disk} given twice")
        entries.append((next_id, folders[tuple(parts[:-1])], False, parts[-1], Path(host).read_bytes()))
        next_id += 1

    image = bytearray(args.size)
    block = 2  # Blocks 0 and 1 are the superblock slots.
    meta = bytearray(struct.pack("<I", len(entries)))
    for ident, parent, is_dir, name, data in entries:
        extents = []
        if not is_dir and data:
            blocks = -(-len(data) // BLOCK)
            if block + blocks > total:
                raise SystemExit("the files do not fit on a disk of this size")
            image[block * BLOCK:block * BLOCK + len(data)] = data
            extents.append((block, blocks))
            block += blocks
        encoded = name.encode()
        meta += struct.pack("<IIBH", ident, parent, int(is_dir), len(encoded)) + encoded
        meta += struct.pack("<QQQI", 0, 0 if is_dir else len(data), 0 if is_dir else checksum(data), len(extents))
        for start, count in extents:
            meta += struct.pack("<QI", start, count)
    meta_blocks = -(-len(meta) // BLOCK)
    if block + meta_blocks > total:
        raise SystemExit("the metadata does not fit on a disk of this size")
    meta_start = block
    image[meta_start * BLOCK:meta_start * BLOCK + len(meta)] = meta
    used = 2 + (block - 2) + meta_blocks
    generation = 1
    superblock = bytearray(BLOCK)
    superblock[:8] = MAGIC
    struct.pack_into("<IIQQQQQQ", superblock, 8, VERSION, BLOCK, generation, total, meta_start, len(meta), checksum(meta), used)
    struct.pack_into("<Q", superblock, 64, checksum(superblock[:64]))
    slot = generation % 2
    image[slot * BLOCK:(slot + 1) * BLOCK] = superblock
    args.image.write_bytes(image)
    files = sum(1 for e in entries if not e[2])
    print(f"{args.image}: {files} file(s), {len(entries) - files} folder(s), {block - 2} data block(s), generation {generation}")


if __name__ == "__main__":
    main()
