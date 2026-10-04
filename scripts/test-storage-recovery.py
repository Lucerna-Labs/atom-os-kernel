#!/usr/bin/env python3
"""Interrupt actual virtio I/O at QEMU blkdebug breakpoints, then cold-boot."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import sys
import time
import uuid

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("atom_boot_test", Path(__file__).with_name("boot-test.py"))
boot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boot)


def checksum(data):
    value = 0xcbf29ce484222325
    for byte in data: value = ((value ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return value


BLOCK = 4096


def latest_snapshot(disk):
    """Independent host-side decoder for the ATOMFS02 copy-on-write format: the
    newest superblock whose metadata and file contents all check out. Returns
    (generation, {root file name: bytes}); invalid generations are skipped."""
    data = disk.read_bytes()
    total = len(data) // BLOCK
    candidates = []
    for slot in range(2):
        block = data[slot * BLOCK:(slot + 1) * BLOCK]
        if block[:8] != b"ATOMFS02": continue
        version, block_size = struct.unpack_from("<II", block, 8)
        generation, blocks, meta_start, meta_len, meta_sum, _used, sb_sum = struct.unpack_from("<QQQQQQQ", block, 16)
        if version != 2 or block_size != BLOCK or checksum(block[:64]) != sb_sum: continue
        if blocks != total or generation == 0 or meta_start < 2 or not 4 <= meta_len <= 64 << 20: continue
        meta = data[meta_start * BLOCK:meta_start * BLOCK + meta_len]
        if len(meta) != meta_len or checksum(meta) != meta_sum: continue
        try:
            count = struct.unpack_from("<I", meta)[0]; at = 4
            nodes = {0: ("", True)}; files = {}
            for _ in range(count):
                ident, parent, is_dir, name_len = struct.unpack_from("<IIBH", meta, at); at += 11
                name = meta[at:at + name_len].decode(); at += name_len
                _modified, size, content_sum, extent_count = struct.unpack_from("<QQQI", meta, at); at += 28
                extents = []
                for _ in range(extent_count):
                    start, run = struct.unpack_from("<QI", meta, at); at += 12
                    extents.append((start, run))
                if parent not in nodes or not nodes[parent][1] or ident in nodes: raise ValueError("tree")
                nodes[ident] = (name, bool(is_dir))
                if is_dir: continue
                content = b"".join(data[start * BLOCK:(start + run) * BLOCK] for start, run in extents)[:size]
                if len(content) != size or checksum(content) != content_sum: raise ValueError("contents")
                if parent == 0: files[name] = content
            if at != meta_len: raise ValueError("length")
            candidates.append((generation, files))
        except (ValueError, UnicodeError, struct.error): continue
    if not candidates: raise AssertionError("no complete generation survived")
    generation, files = max(candidates, key=lambda candidate: candidate[0])
    return generation, files


def probe_files(files, nonce):
    """Only the storage probe's files (the disk also holds e.g. boot.done)."""
    return {name: content for name, content in files.items() if name.startswith(f"r-{nonce}-")}


def expected_files(nonce, next_generation):
    names = [f"r-{nonce}-{i}.bin" for i in range(8)]
    return {name: bytes((offset * 17 + index * 43 + ord(nonce[index % len(nonce)])
                        + (101 if next_generation else 0)) % 251
                       for offset in range(65536))
            for index, name in enumerate(names)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="kvm")
    args = parser.parse_args()
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    source = args.source.resolve()
    nonce = uuid.uuid4().hex[:16]
    old, new = expected_files(nonce, False), expected_files(nonce, True)
    image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    result = {"image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(), "acceleration": args.accel,
              "nonce": nonce, "probe_bytes": 524288, "files": 8, "disk_format": "ATOMFS02", "cases": {}, "success": False}
    started = time.monotonic(); guest = None
    try:
        seed = output / "seed.img"
        with seed.open("xb") as stream: stream.truncate(8 * 1024 * 1024)
        guest = boot.Guest(source, output / "seed", seed, args.accel)
        guest.ready()
        guest.command(f"storageprobe seed {nonce}", "STORAGE_SEED_READY files=8 bytes=524288", 90)
        guest.command("sync", "SYNC_OK", 60)
        guest.close(); guest = None
        generation, files = latest_snapshot(seed)
        assert generation == 1 and probe_files(files, nonce) == old
        result["seed_generation"] = generation
        print("SEED_SAVED PASS", flush=True)

        # A save of the eight changed files: 8 data writes, flush, 1 metadata
        # write, flush, 1 superblock write, flush. Each cut suspends the request
        # reached by stepping through these events; the value is the completed
        # (writes, flushes) expected at that point.
        cut_points = {
            "data-write": (["write_aio"], (0, 0)),
            "data-flush": (["flush_to_disk"], (8, 0)),
            "metadata-write": (["flush_to_disk", "write_aio"], (8, 1)),
            "metadata-flush": (["flush_to_disk", "flush_to_disk"], (9, 1)),
            "superblock-write": (["flush_to_disk", "flush_to_disk", "write_aio"], (9, 2)),
            "superblock-flush": (["flush_to_disk", "flush_to_disk", "flush_to_disk"], (10, 2)),
        }
        for label, (steps, expected) in cut_points.items():
            case = output / label; case.mkdir()
            disk = case / "data.img"; shutil.copyfile(seed, disk)
            config = case / "blkdebug.conf"; config.write_text("# Breakpoints are armed through QMP.\n")
            guest = boot.Guest(source, case / "running", disk, args.accel, blkdebug=config)
            guest.ready()
            guest.command(f"storageprobe mutate {nonce}", "STORAGE_NEXT_READY files=8 bytes=524288", 90)
            before = guest.block_stats()
            guest.block_command(f"break {steps[0]} step0")
            offset = len(guest.serial())
            guest.keys("sync\n")
            guest.wait_block("step0")
            for index, event in enumerate(steps[1:], 1):
                guest.block_command(f"break {event} step{index}")
                guest.block_command(f"resume step{index - 1}")
                guest.wait_block(f"step{index}")
            after = guest.block_stats()
            observed = (after["wr_operations"] - before["wr_operations"],
                        after["flush_operations"] - before["flush_operations"])
            assert observed == expected, (label, observed, expected, after)
            assert "SYNC_OK" not in guest.serial()[offset:]
            (case / "suspended.json").write_text(json.dumps({"phase": label, "before": before, "after": after,
                "completed_write_flush_delta": observed, "expected_delta": expected}, indent=2) + "\n")
            guest.abrupt_stop(); guest.close(); guest = None
            recovered_generation, recovered = latest_snapshot(disk)
            recovered = probe_files(recovered, nonce)
            assert recovered in (old, new), f"mixed or damaged generation after {label}"
            phase = "old" if recovered == old else "new"
            assert recovered_generation == (1 if phase == "old" else 2)
            guest = boot.Guest(source, case / "cold", disk, args.accel)
            guest.ready()
            guest.command(f"storageprobe verify {nonce}", f"STORAGE_VERIFY_OK phase={phase} files=8 bytes=524288 dirty=0", 90)
            guest.command("help", "commands:")
            guest.close(); guest = None
            result["cases"][label] = {"passed": True, "completed_write_flush_delta": observed,
                "recovered_generation": recovered_generation, "recovered_phase": phase,
                "disk_sha256": hashlib.sha256(disk.read_bytes()).hexdigest()}
            print(label.upper() + "_INTERRUPTION PASS", flush=True)

        for label, event, errno in [("write-error", "write_aio", 5), ("flush-error", "flush_to_disk", 5),
                                    ("host-no-space", "write_aio", 28)]:
            case = output / label; case.mkdir()
            disk = case / "data.img"; shutil.copyfile(seed, disk)
            config = case / "blkdebug.conf"
            config.write_text(f'[inject-error]\nevent = "{event}"\nerrno = "{errno}"\nonce = "on"\n')
            guest = boot.Guest(source, case / "running", disk, args.accel, blkdebug=config)
            guest.ready()
            guest.command(f"storageprobe mutate {nonce}", "STORAGE_NEXT_READY files=8 bytes=524288", 90)
            guest.command("sync", "SYNC_FAILED", 60)
            guest.command("status", r"FS_STATUS unsaved .*generation=1 disk=1")
            generation, files = latest_snapshot(disk)
            assert generation == 1 and probe_files(files, nonce) == old
            guest.command("help", "commands:")
            guest.command("sync", "SYNC_OK", 60)
            guest.command("status", r"FS_STATUS saved .*generation=2 disk=1")
            guest.close(); guest = None
            generation, files = latest_snapshot(disk)
            assert generation == 2 and probe_files(files, nonce) == new
            guest = boot.Guest(source, case / "cold", disk, args.accel)
            guest.ready()
            guest.command(f"storageprobe verify {nonce}", "STORAGE_VERIFY_OK phase=new files=8 bytes=524288 dirty=0", 90)
            guest.close(); guest = None
            result["cases"][label] = {"passed": True, "injected_errno": errno, "retry_generation": generation,
                                      "previous_acknowledged_snapshot_preserved": True}
            print(label.upper() + "_RECOVERY PASS", flush=True)
        result["success"] = True
    except Exception as error:
        result["error"] = repr(error)
        print("FAIL: " + repr(error), flush=True)
    finally:
        if guest:
            try:
                # A stuck breakpoint must not trap graceful QEMU shutdown.
                guest.abrupt_stop()
            except Exception: pass
            guest.close()
        result["elapsed_seconds"] = round(time.monotonic() - started, 3)
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
