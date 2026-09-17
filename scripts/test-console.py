#!/usr/bin/env python3
"""Exercise the real interactive launcher through a PTY, retaining its disk."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import subprocess
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="kvm")
    args = parser.parse_args()
    root = args.source.resolve(); output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    disk = output / "continuing-session.img"
    token = "console-" + uuid.uuid4().hex[:16]
    result = {"nonce": token, "success": False, "sessions": [], "acceleration": args.accel}
    previous_hash = None
    try:
        for number in [1, 2]:
            master, slave = pty.openpty()
            pidfile = output / f"qemu-{number}.pid"
            log = output / f"console-{number}.log"
            command = ["python3", str(root / "scripts/run.py"), "--accel", args.accel, "--disk", str(disk), "--pidfile", str(pidfile)]
            process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
            os.close(slave)
            content = bytearray()

            def read_until(pattern, seconds=30, offset=0):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    text = content.decode(errors="replace").replace("\r", "")
                    if re.search(pattern, text[offset:]): return text
                    if select.select([master], [], [], 0.1)[0]:
                        try: data = os.read(master, 65536)
                        except OSError: data = b""
                        if data: content.extend(data); log.write_bytes(content)
                    if process.poll() is not None: break
                raise RuntimeError(f"missing {pattern!r}; terminal tail: {content[-2500:].decode(errors='replace')}")

            def send(text, expected):
                offset = len(content.decode(errors="replace").replace("\r", ""))
                os.write(master, (text + "\r").encode())
                return read_until(expected, offset=offset)

            try:
                read_until(r"10,000 SYS_YIELDs took \(CPU cycles\): \d+\n> ")
                send("help", "commands:")
                if number == 1:
                    send(f"echo {token} > console.txt", "> ")
                    send("mv console.txt retained.txt", "RENAMED")
                    send("sync", "SYNC_OK")
                    send("status", r"FS_STATUS saved .*generation=1 disk=1")
                    previous_hash = hashlib.sha256(disk.read_bytes()).hexdigest()
                else:
                    assert hashlib.sha256(disk.read_bytes()).hexdigest() == previous_hash, "launcher altered existing disk on startup"
                    send("cat retained.txt", re.escape(token) + r"\n")
                    send("status", r"FS_STATUS saved .*generation=1 disk=1")
                    # A second launcher must refuse to share this live image.
                    duplicate = subprocess.run(command[:-2], capture_output=True, text=True, timeout=5)
                    assert duplicate.returncode != 0 and "already using" in duplicate.stderr
                    result["exclusive_session_lock"] = True
                os.write(master, b"\x01x")
                process.wait(timeout=10)
                assert process.returncode == 0, process.returncode
                assert hashlib.sha256(disk.read_bytes()).hexdigest() == previous_hash
                result["sessions"].append({"number": number, "passed": True, "disk_sha256": previous_hash})
                print(f"INTERACTIVE_SESSION_{number} PASS", flush=True)
            finally:
                log.write_bytes(content)
                if process.poll() is None:
                    if pidfile.exists():
                        pid = int(pidfile.read_text())
                        try:
                            actual = Path(f"/proc/{pid}/cmdline").read_bytes()
                            if b"qemu-system" in actual and str(disk).encode() in actual: os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError: pass
                    process.terminate()
                    try: process.wait(timeout=5)
                    except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
                os.close(master)
        result["success"] = True
    except Exception as error:
        result["error"] = repr(error)
        print("FAIL: " + repr(error), flush=True)
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
