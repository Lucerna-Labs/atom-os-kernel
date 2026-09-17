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


def latest_snapshot(disk):
    """Independent host-side decoder; invalid or incomplete slots are skipped."""
    candidates = []
    with disk.open("rb") as image:
        for slot in range(2):
            base = slot * 1025 * 512
            image.seek(base); header = image.read(512)
            if header[:8] != b"ATOMFS01": continue
            version, length = struct.unpack_from("<II", header, 8)
            generation, payload_hash = struct.unpack_from("<QQ", header, 16)
            count = struct.unpack_from("<I", header, 32)[0]
            if version != 1 or not 4 <= length <= 524288 or generation == 0 or count > 128: continue
            if checksum(header[:40]) != struct.unpack_from("<Q", header, 40)[0]: continue
            payload = image.read(length)
            if len(payload) != length or checksum(payload) != payload_hash: continue
            try:
                if struct.unpack_from("<I", payload)[0] != count: continue
                cursor = 4; files = {}
                for _ in range(count):
                    n, size = struct.unpack_from("<HI", payload, cursor); cursor += 6
                    if not 1 <= n <= 63 or size > 65536 or cursor + n + size > length: raise ValueError()
                    name = payload[cursor:cursor + n].decode(); cursor += n
                    if name in files: raise ValueError()
                    files[name] = payload[cursor:cursor + size]; cursor += size
                if cursor != length: continue
                candidates.append((generation, files))
            except (ValueError, UnicodeError, struct.error): continue
    if not candidates: raise AssertionError("no complete snapshot survived")
    return max(candidates, key=lambda candidate: candidate[0])


def expected_files(nonce, next_generation):
    names = [f"r-{nonce}-{i}.bin" for i in range(8)]
    tail = 524288 - 4 - sum(6 + len(name) for name in names) - 7 * 65536
    return {name: bytes((offset * 17 + index * 43 + ord(nonce[index % len(nonce)])
                        + (101 if next_generation else 0)) % 251
                       for offset in range(65536 if index < 7 else tail))
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
              "nonce": nonce, "snapshot_bytes": 524288, "files": 8, "cases": {}, "success": False}
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
        assert generation == 1 and files == old
        result["seed_generation"] = generation
        print("SEED_AT_FULL_CAPACITY PASS", flush=True)

        # Expected completed device operations at each suspended request.
        cut_points = {"payload-write": (0, 0), "payload-flush": (1024, 0),
                      "header-write": (1024, 1), "header-flush": (1025, 1)}
        for label, expected in cut_points.items():
            case = output / label; case.mkdir()
            disk = case / "data.img"; shutil.copyfile(seed, disk)
            config = case / "blkdebug.conf"; config.write_text("# Breakpoints are armed through QMP.\n")
            guest = boot.Guest(source, case / "running", disk, args.accel, blkdebug=config)
            guest.ready()
            guest.command(f"storageprobe mutate {nonce}", "STORAGE_NEXT_READY files=8 bytes=524288", 90)
            before = guest.block_stats()
            event = "write_aio" if label == "payload-write" else "flush_to_disk"
            guest.block_command(f"break {event} first")
            offset = len(guest.serial())
            guest.keys("sync\n")
            guest.wait_block("first")
            if label.startswith("header-"):
                next_event = "write_aio" if label == "header-write" else "flush_to_disk"
                guest.block_command(f"break {next_event} final")
                guest.block_command("resume first")
                guest.wait_block("final")
            after = guest.block_stats()
            observed = (after["wr_operations"] - before["wr_operations"],
                        after["flush_operations"] - before["flush_operations"])
            assert observed == expected, (label, observed, expected, after)
            assert "SYNC_OK" not in guest.serial()[offset:]
            (case / "suspended.json").write_text(json.dumps({"phase": label, "before": before, "after": after,
                "completed_write_flush_delta": observed, "expected_delta": expected}, indent=2) + "\n")
            guest.abrupt_stop(); guest.close(); guest = None
            recovered_generation, recovered = latest_snapshot(disk)
            assert recovered in (old, new), f"mixed or damaged snapshot after {label}"
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
            assert generation == 1 and files == old
            guest.command("help", "commands:")
            guest.command("sync", "SYNC_OK", 60)
            guest.command("status", r"FS_STATUS saved .*generation=2 disk=1")
            guest.close(); guest = None
            generation, files = latest_snapshot(disk)
            assert generation == 2 and files == new
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
