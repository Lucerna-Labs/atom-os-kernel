#!/usr/bin/env python3
"""Boot the production image and test imported runtime facilities, using an isolated disk."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import sys
import time
import uuid

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("atom_boot", Path(__file__).with_name("boot-test.py"))
boot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boot)


def checksum(data):
    value = 0xcbf29ce484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return value


def prepare_disk(path):
    # A disk in the earlier snapshot format (ATOMFS01, folder-tree payload) with
    # one file, so every run also exercises the importer. This affects only
    # this newly created test disk, never the user's disk.
    name, data = b"imported.txt", b"userspace import acceptance\n"
    payload = b"ATOMFST2" + struct.pack("<I", 1)
    payload += struct.pack("<BHI", 2, len(name), len(data)) + name + data
    header = bytearray(512)
    header[:8] = b"ATOMFS01"
    struct.pack_into("<IIQQI", header, 8, 2, len(payload), 1, checksum(payload), 1)
    struct.pack_into("<Q", header, 40, checksum(header[:40]))
    with path.open("xb") as disk:
        disk.truncate(8 * 1024 * 1024)
        disk.write(header)
        disk.write(payload)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="tcg")
    args = parser.parse_args()
    source, out = args.source.resolve(), args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    disk = out / "data.img"
    prepare_disk(disk)
    token = "buffered-" + uuid.uuid4().hex[:16]
    image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    result = {"success": False, "acceleration": args.accel, "nonce": token,
              "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
              "fixture": "isolated ATOMFS01 v2 disk with one file (imported on boot)", "checks": {}}
    guest = None
    started = time.monotonic()
    def passed(name):
        result["checks"][name] = True
        print(name + " PASS", flush=True)
    try:
        guest = boot.Guest(source, out / "first", disk, args.accel)
        result["qmp_kvm"] = guest.kvm
        guest.wait("shell: ready")
        guest.wait(r"STORAGE_READY generation=\d+")
        passed("BOOT_INTERACTIVE_CURRENT_IMAGE")
        match = guest.command("spawn worker.elf --ipc-test", r"spawned pid (\d+)")
        child = int(match[1])
        gate = guest.wait(r"IPC_BUFFER_OK rounds=32 free_before=(\d+) free_after=(\d+)", seconds=90)
        assert gate[1] == gate[2]
        for marker in ["IPC_SHORT_BUFFER_PRESERVES_QUEUE_OK", "IPC_EMPTY_MAXIMUM_UTF8_OWNERSHIP_OK", "IPC_FIFO_BACKPRESSURE_OK"]:
            guest.wait(marker)
            passed(marker)
        result["free_frames"] = {"before": int(gate[1]), "after": int(gate[2])}
        guest.command(f"wait {child}", rf"wait pid={child} status=0", 90)
        passed("BUFFERED_IPC_NO_FRAME_LEAK")
        guest.command(f"msg {token}", re.escape("[Daemon] Received IPC: " + token), 60)
        passed("DAEMON_USES_BUFFERED_RECEIVE")
        guest.command("mkdir imports", "folder created")
        guest.command(f"echo {token} > imports/note", r"/> ")
        guest.command("sync", "SYNC_OK")
        guest.close(); guest = None
        guest = boot.Guest(source, out / "cold", disk, args.accel)
        guest.wait("shell: ready")
        guest.command("cat imports/note", re.escape(token) + r"\n")
        passed("DIRECTORY_FILE_COLD_BOOT_PERSISTENCE")
        match = guest.command("spawn hello.elf", r"spawned pid (\d+)")
        child = int(match[1])
        guest.command(f"wait {child}", rf"wait pid={child} status=0")
        passed("PREFLIGHT_ACCEPTED_PROGRAM_EXECUTES")
        result["success"] = True
    except Exception as error:
        result["error"] = str(error)
        print("FAIL: " + str(error), flush=True)
    finally:
        if guest:
            guest.close()
        result["elapsed_seconds"] = round(time.monotonic() - started, 3)
        (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
