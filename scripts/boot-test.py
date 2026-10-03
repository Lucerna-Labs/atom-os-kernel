#!/usr/bin/env python3
"""Real QEMU/KVM acceptance: process lifecycle, memory, IPC and durable reboot."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import time
import uuid


class Guest:
    def __init__(self, source, output, disk, accel, memory):
        self.output = output
        output.mkdir()
        image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
        command = ["qemu-system-x86_64", "-accel", accel, "-m", memory, "-smp", "1",
                   "-drive", f"format=raw,file={image},snapshot=on",
                   "-drive", f"if=none,format=raw,file={disk},id=atomdata,cache=writeback",
                   "-device", "virtio-blk-pci,drive=atomdata,disable-modern=on",
                   "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
                   "-display", "none", "-vga", "none", "-nic", "none", "-serial", f"file:{output / 'serial.log'}",
                   "-qmp", f"unix:{output / 'qmp.sock'},server=on,wait=off",
                   "-no-shutdown", "-d", "guest_errors", "-D", str(output / "guest-errors.log")]
        if accel == "kvm" and not os.access("/dev/kvm", os.R_OK | os.W_OK):
            command = ["sudo", "-n", "setpriv", f"--reuid={os.getuid()}", "--regid=kvm", "--init-groups"] + command
        (output / "command.json").write_text(json.dumps(command, indent=2) + "\n")
        self.events = (output / "qmp.jsonl").open("w")
        self.errors = (output / "qemu.stderr.log").open("w")
        self.process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=self.errors)
        self.socket = None
        self.wire = None
        try:
            deadline = time.monotonic() + 10
            while not (output / "qmp.sock").exists() and time.monotonic() < deadline:
                if self.process.poll() is not None: raise RuntimeError("QEMU launch failed")
                time.sleep(0.05)
            self.socket = socket.socket(socket.AF_UNIX)
            self.socket.settimeout(5)
            self.socket.connect(str(output / "qmp.sock"))
            self.wire = self.socket.makefile("rwb")
            self.events.write(self.wire.readline().decode())
            self.qmp("qmp_capabilities")
            self.kvm = self.qmp("query-kvm")
            if accel == "kvm" and not self.kvm["enabled"]: raise RuntimeError("KVM was requested but is not enabled")
        except Exception:
            self.close()
            raise

    def qmp(self, name, arguments=None):
        request = {"execute": name}
        if arguments is not None: request["arguments"] = arguments
        self.events.write(json.dumps({"request": request}) + "\n")
        self.wire.write(json.dumps(request).encode() + b"\n")
        self.wire.flush()
        while True:
            line = self.wire.readline()
            if not line: raise RuntimeError("QMP disconnected")
            self.events.write(line.decode())
            response = json.loads(line)
            if "event" in response: continue
            if "error" in response: raise RuntimeError(str(response))
            return response["return"]

    def serial(self):
        file = self.output / "serial.log"
        return file.read_text(errors="replace").replace("\r", "") if file.exists() else ""

    def wait(self, expression, offset=0, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            data = self.serial()[offset:]
            if re.search(r"KERNEL_EXCEPTION|KERNEL_PANIC|ALLOCATION ERROR|USER_PANIC|USER_OOM", data):
                raise RuntimeError("kernel or user fault: " + data[-2000:])
            match = re.search(expression, data)
            if match: return match
            if self.process.poll() is not None: raise RuntimeError(f"QEMU exited {self.process.returncode}: {data[-2000:]}")
            time.sleep(0.05)
        raise RuntimeError(f"missing {expression!r}; serial tail: {self.serial()[-2500:]}")

    def keys(self, text):
        mapping = {" ": "spc", "\n": "ret", ".": "dot", ">": "kp_add", "-": "minus", "/": "slash"}
        for character in text:
            self.qmp("send-key", {"keys": [{"type": "qcode", "data": mapping.get(character, character)}], "hold-time": 20})
            time.sleep(0.04)

    def command(self, text, expected, seconds=30):
        offset = len(self.serial())
        self.keys(text + "\n")
        return self.wait(expected, offset, seconds)

    def ready(self, offset=0):
        self.wait(r"10,000 SYS_YIELDs took \(CPU cycles\): \d+\n> ", offset)
        self.wait(r"HEAP_OK", offset)
        self.wait(r"STORAGE_READY generation=\d+", offset)

    def close(self):
        if self.process.poll() is None:
            if self.wire:
                try: self.qmp("quit")
                except Exception: pass
            try: self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                try: self.process.wait(timeout=5)
                except subprocess.TimeoutExpired: self.process.kill(); self.process.wait(timeout=5)
        if self.wire: self.wire.close()
        if self.socket: self.socket.close()
        self.errors.close(); self.events.close()
        (self.output / "qmp.sock").unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="tcg")
    parser.add_argument("--rounds", type=int, default=48)
    parser.add_argument("--memory", default="8G", help="guest RAM, in QEMU -m syntax (default 8G)")
    args = parser.parse_args()
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    source = args.source.resolve()
    disk = output / "data.img"
    with disk.open("xb") as stream: stream.truncate(64 * 1024 * 1024)
    image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    token = "persist-" + uuid.uuid4().hex[:16]
    result = {"acceleration": args.accel, "memory": args.memory, "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
              "nonce": token, "checks": {}, "disk": str(disk)}
    started = time.monotonic()
    guest = None

    def passed(name):
        result["checks"][name] = True
        print(name + " PASS", flush=True)

    try:
        guest = Guest(source, output / "first", disk, args.accel, args.memory)
        result["qmp_kvm"] = guest.kvm
        guest.ready(); passed("BOOT_HEAP_BENCH")
        match = guest.wait(r"MEMORY_READY frames=(\d+) mib=(\d+) regions=(\d+) top=(0x[0-9a-f]+) probe=ok")
        guest_mib = guest.qmp("query-memory-size-summary")["base-memory"] >> 20
        result["memory_ready"] = {"frames": int(match[1]), "mib": int(match[2]), "regions": int(match[3]), "top": match[4], "guest_mib": guest_mib}
        # All but firmware, the first 1 MiB and the boot image's own frames.
        assert int(match[2]) >= guest_mib - 64, match[0]
        passed("PHYSICAL_MEMORY_DISCOVERED")
        guest.command("help", "commands:"); passed("KEYBOARD")
        guest.command("ls /bin", r"daemon\.elf[\s\S]*shell\.elf"); passed("RAMFS_LIST")
        guest.command(f"echo {token} > durable.txt", "> ")
        guest.command("cat durable.txt", re.escape(token) + r"\n"); passed("RAMFS_WRITE_READ")
        guest.command("msg acceptance ipc", r"Received IPC: acceptance ipc"); passed("IPC_DELIVERY")
        guest.command("heaptest", "HEAP_OK"); passed("USER_HEAP")
        guest.command("selftest", "SELFTEST_OK", 60); passed("USER_POINTER_FD_FASTCALL_XMM")
        guest.command("pairtest", "PAIRTEST_OK", 60); passed("CONCURRENT_PROCESS_ISOLATION")
        match = guest.command("spawn worker.elf", r"spawned pid (\d+)")
        pid = int(match[1])
        guest.command(f"wait {pid}", rf"wait pid={pid} status=37", 60)
        guest.command(f"wait {pid}", "wait failed"); passed("SPAWN_WAIT_EXIT")
        guest.command("echo invalid > malformed.elf", "> ")
        guest.command("spawn malformed.elf", "spawn failed"); passed("INVALID_ELF_REJECTED")
        match = guest.command(f"churn {args.rounds}", r"CHURN_OK rounds=(\d+) free_before=(\d+) free_after=(\d+) tasks=(\d+)", 120)
        assert int(match[1]) == args.rounds and match[2] == match[3] and match[4] == "2"
        result["process_churn"] = {"rounds": int(match[1]), "free_before": int(match[2]), "free_after": int(match[3]), "tasks": int(match[4])}
        passed("PROCESS_REAP_NO_FRAME_LEAK")
        match = guest.command("spawn sleeper.elf", r"spawned pid (\d+)")
        sleeper = int(match[1])
        guest.command("ps", rf"\n{sleeper} +1 +\w+ +sleeper\.elf\n")
        guest.command(f"kill {sleeper}", rf"killed pid {sleeper}")
        guest.command(f"wait {sleeper}", rf"wait pid={sleeper} status=137")
        guest.command(f"kill {sleeper}", "kill failed")
        guest.command("kill 9999", "kill failed")
        match = guest.command("stats", r"FREE_FRAMES (\d+) TASKS (\d+)")
        assert int(match[1]) == result["process_churn"]["free_after"] and match[2] == "2", match[0]
        listing = guest.command("ps", r"PID PARENT STATE +NAME\n[\s\S]*?\n> ")
        assert "sleeper" not in listing[0], listing[0]
        passed("PS_KILL_REAP")
        guest.command("echo doomed > doomed.txt", "> ")
        guest.command("mv doomed.txt kept.txt", "renamed")
        listing = guest.command("ls", r"bin/\n[\s\S]*?\n> ")
        assert "kept.txt" in listing[0] and "doomed.txt" not in listing[0], listing[0]
        guest.command("cat kept.txt", r"doomed\n")
        guest.command("echo spare > spare.txt", "> ")
        guest.command("mv kept.txt spare.txt", "mv failed")
        guest.command("rm spare.txt", "removed")
        guest.command("rm worker.elf", "rm failed")
        guest.command("rm bin/worker.elf", "rm failed: read-only")
        guest.command("mv shell.elf x.elf", "mv failed")
        guest.command("mv bin/shell.elf x.elf", "mv failed: read-only")
        passed("FILE_REMOVE_RENAME")
        guest.command("mkdir docs", "folder created")
        guest.command("cd docs", "> ")
        guest.command("pwd", r"\n/docs\n")
        guest.command("echo inner > note.txt", "> ")
        guest.command("cd /", "> ")
        guest.command("cat docs/note.txt", r"inner\n")
        guest.command("mkdir docs/deep", "folder created")
        guest.command("mv docs/note.txt docs/deep", "renamed")
        guest.command("ls docs/deep", r"note\.txt")
        guest.command("rmdir docs", "rmdir failed: folder is not empty")
        guest.command("cd docs/missing", "cd: not found")
        passed("FOLDERS")
        # 3 MB through the bulk read/write calls: far past the old 64 KiB file limit.
        match = guest.command("fill docs/big.bin 3000000", r"FILL_OK bytes=3000000 sum=([0-9a-f]{16})", 60)
        big_sum = match[1]
        guest.command("sum docs/big.bin", rf"SUM bytes=3000000 sum={big_sum}", 60)
        guest.command("cp docs/big.bin copy.bin", r"copied 2\.8 MB", 60)
        guest.command("sum copy.bin", rf"SUM bytes=3000000 sum={big_sum}", 60)
        guest.command("df", r"Unsaved changes")
        result["large_file_sum"] = big_sum
        passed("LARGE_FILE_BULK_IO")
        guest.command("faulttest", "FAULT_ISOLATION_OK status=142"); passed("USER_FAULT_CONTAINED")
        guest.command("sync", "SYNC_OK", 120)
        guest.command("df", r"Everything is saved"); passed("VIRTIO_FLUSH_COMMIT")
        offset = len(guest.serial())
        guest.keys("reboot\n")
        guest.ready(offset)
        guest.command("cat durable.txt", re.escape(token) + r"\n"); passed("OS_REBOOT_PERSISTENCE")
        guest.command("cat kept.txt", r"doomed\n")
        listing = guest.command("ls", r"bin/\n[\s\S]*?\n> ")
        assert "spare.txt" not in listing[0] and "doomed.txt" not in listing[0], listing[0]
        passed("REMOVE_RENAME_PERSISTENCE")
        guest.command("cat docs/deep/note.txt", r"inner\n")
        guest.command("sum docs/big.bin", rf"SUM bytes=3000000 sum={big_sum}", 60)
        guest.command("sum copy.bin", rf"SUM bytes=3000000 sum={big_sum}", 60)
        passed("FOLDERS_AND_LARGE_FILES_PERSIST")
        second = "second-" + token
        guest.command(f"echo {second} > durable.txt", "> ")
        guest.command("sync", "SYNC_OK")
        guest.close(); guest = None
        result["disk_sha256_after_save"] = hashlib.sha256(disk.read_bytes()).hexdigest()
        guest = Guest(source, output / "cold", disk, args.accel, args.memory)
        guest.ready()
        guest.command("cat durable.txt", re.escape(token + "\n" + second + "\n")); passed("COLD_BOOT_EXACT_CONTENT")
        offset = len(guest.serial())
        guest.keys("run worker.elf\n")
        guest.wait("WORKER_OK", offset, 60)
        guest.wait(r"\[Daemon\] Heartbeat\.\.\. tasks=1", offset, 10)
        passed("EXEC_REPLACEMENT_DAEMON_SURVIVES")
        guest.close(); guest = None
        result["success"] = True
    except Exception as error:
        result["success"] = False
        result["error"] = str(error)
        print("FAIL: " + str(error), flush=True)
        if guest and guest.process.poll() is None:
            try:
                regs = guest.qmp("human-monitor-command", {"command-line": "info registers"})
                (guest.output / "registers.txt").write_text(regs)
            except Exception: pass
    finally:
        if guest: guest.close()
        result["elapsed_seconds"] = round(time.monotonic() - started, 3)
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
