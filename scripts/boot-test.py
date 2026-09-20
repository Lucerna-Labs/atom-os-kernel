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
import tempfile
import signal
import time
import uuid


class Guest:
    def __init__(self, source, output, disk, accel, *, blkdebug=None):
        self.output = output
        output.mkdir()
        self.control = tempfile.TemporaryDirectory(prefix="atom-qmp-")
        self.qmp_path = Path(self.control.name) / "qmp.sock"
        self.pidfile = output / "qemu.pid"
        self.disk = disk
        image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
        disk_source = str(disk) if blkdebug is None else f"blkdebug:{blkdebug}:{disk}"
        command = ["qemu-system-x86_64", "-accel", accel, "-m", "128M", "-smp", "1",
                   "-drive", f"format=raw,file={image},snapshot=on",
                   "-drive", f"if=none,format=raw,file={disk_source},id=atomdata,cache=writeback,werror=report,rerror=report",
                   "-device", "virtio-blk-pci,drive=atomdata,disable-modern=on",
                   "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
                   "-display", "none", "-nic", "none", "-serial", f"file:{output / 'serial.log'}",
                   "-qmp", f"unix:{self.qmp_path},server=on,wait=off", "-pidfile", str(self.pidfile),
                   "-no-shutdown", "-d", "guest_errors", "-D", str(output / "guest-errors.log")]
        if blkdebug is not None:
            command = ["stdbuf", "-oL"] + command
        if accel == "kvm" and not os.access("/dev/kvm", os.R_OK | os.W_OK):
            command = ["sudo", "-n", "setpriv", f"--reuid={os.getuid()}", "--regid=kvm", "--init-groups"] + command
        (output / "command.json").write_text(json.dumps(command, indent=2) + "\n")
        self.events = (output / "qmp.jsonl").open("w")
        self.errors = (output / "qemu.stderr.log").open("w")
        self.stdout = (output / "qemu.stdout.log").open("w")
        self.process = subprocess.Popen(command, stdout=self.stdout, stderr=self.errors)
        self.socket = None
        self.wire = None
        try:
            deadline = time.monotonic() + 10
            while not self.qmp_path.exists() and time.monotonic() < deadline:
                if self.process.poll() is not None: raise RuntimeError("QEMU launch failed")
                time.sleep(0.05)
            self.socket = socket.socket(socket.AF_UNIX)
            self.socket.settimeout(5)
            self.socket.connect(str(self.qmp_path))
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

    def block_command(self, command):
        response = self.qmp("human-monitor-command", {"command-line": f'qemu-io atomdata "{command}"'})
        if re.search(r"could not|error|not found|invalid|unknown", response, re.I):
            raise RuntimeError(f"block debug command failed: {response}")
        return response

    def block_stats(self):
        return next(d["stats"] for d in self.qmp("query-blockstats") if d.get("device") == "atomdata")

    def wait_block(self, tag, seconds=30):
        # HMP qemu-io wait_break can occupy the monitor while the guest is still
        # submitting requests. Observe QEMU's line-buffered suspension event
        # first, then ask the block driver to confirm that exact suspended tag.
        deadline = time.monotonic() + seconds
        marker = f"blkdebug: Suspended request '{tag}'"
        while time.monotonic() < deadline:
            if marker in (self.output / "qemu.stdout.log").read_text(errors="replace"):
                return self.block_command(f"wait_break {tag}")
            if self.process.poll() is not None: raise RuntimeError("QEMU exited before the block breakpoint")
            time.sleep(0.005)
        raise RuntimeError(f"block breakpoint did not suspend: {tag}; {self.serial()[-1000:]}")

    def abrupt_stop(self):
        pid = int(self.pidfile.read_text())
        command = Path(f"/proc/{pid}/cmdline").read_bytes()
        if b"qemu-system" not in command or str(self.disk).encode() not in command:
            raise RuntimeError("refusing to signal a process outside this test guest")
        os.kill(pid, signal.SIGKILL)
        self.process.wait(timeout=10)
        self.events.write(json.dumps({"host_action": "SIGKILL", "qemu_pid": pid}) + "\n")
        self.events.flush()

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
        self.errors.close(); self.events.close(); self.stdout.close()
        self.qmp_path.unlink(missing_ok=True)
        self.control.cleanup()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="tcg")
    parser.add_argument("--rounds", type=int, default=48)
    args = parser.parse_args()
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    source = args.source.resolve()
    disk = output / "data.img"
    with disk.open("xb") as stream: stream.truncate(8 * 1024 * 1024)
    image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    token = "persist-" + uuid.uuid4().hex[:16]
    result = {"acceleration": args.accel, "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
              "nonce": token, "checks": {}, "disk": str(disk)}
    started = time.monotonic()
    guest = None

    def passed(name):
        result["checks"][name] = True
        print(name + " PASS", flush=True)

    try:
        guest = Guest(source, output / "first", disk, args.accel)
        result["qmp_kvm"] = guest.kvm
        guest.ready(); passed("BOOT_HEAP_BENCH")
        guest.command("fstest", "FSTEST_OK", 90)
        guest.wait("FILE_LIFECYCLE_OK")
        guest.wait("DURABLE_QUOTAS_OK")
        guest.wait("FS_STATE_OK")
        passed("FILE_LIFECYCLE_QUOTAS_SAVED_STATE")
        guest.command("status", r"FS_STATUS saved files=0/128 bytes=4/524288")
        guest.command("echo kept > move.txt", "> ")
        guest.command("mv move.txt renamed.txt", "RENAMED")
        guest.command("cat renamed.txt", r"\nkept\n")
        guest.command("rm renamed.txt", "REMOVED")
        guest.command("cat renamed.txt", "cat: file not found")
        guest.command("df", r"FS_STATUS unsaved files=0/128 bytes=4/524288")
        passed("FILE_MANAGEMENT_COMMANDS")
        guest.command("proctest", "PROCTEST_OK", 90)
        for marker in ["PROCESS_INPUT_VALIDATION_OK", "ARGUMENT_LIMITS_EXEC_OK", "KILL_WAIT_ORPHAN_OK", "KILL_REAP_OK"]:
            guest.wait(marker)
        passed("PROCESS_ARGUMENTS_KILL_BOUNDARIES")
        guest.command("ps", r"1 0 running shell\.elf")
        match = guest.command("spawn worker.elf --sleep", r"spawned pid (\d+)")
        sleeper = int(match[1])
        guest.command("ps", rf"{sleeper} 1 sleeping worker\.elf")
        guest.command(f"kill {sleeper}", rf"killed pid {sleeper}")
        guest.command("ps", rf"{sleeper} 1 exited worker\.elf")
        guest.command(f"wait {sleeper}", rf"wait pid={sleeper} status=137")
        guest.command(f"kill {sleeper}", "kill failed")
        guest.command("kill 0", "kill failed")
        passed("PS_KILL_COMMANDS")
        match = guest.command("spawn worker.elf --args hello world", r"spawned pid (\d+)")
        argument_pid = int(match[1])
        guest.wait(rf"ARGS pid={argument_pid} count=4")
        guest.wait("ARG 2 len=5 hello\nARG 3 len=5 world")
        guest.command(f"wait {argument_pid}", rf"wait pid={argument_pid} status=41")
        passed("SHELL_PROGRAM_ARGUMENTS")
        guest.command("help", "commands:"); passed("KEYBOARD")
        guest.command("ls", r"shell\.elf.*daemon\.elf"); passed("RAMFS_LIST")
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
        guest.command("faulttest", "FAULT_ISOLATION_OK status=142"); passed("USER_FAULT_CONTAINED")
        guest.command("sync", "SYNC_OK"); passed("VIRTIO_FLUSH_COMMIT")
        offset = len(guest.serial())
        guest.keys("reboot\n")
        guest.ready(offset)
        guest.command("cat durable.txt", re.escape(token) + r"\n"); passed("OS_REBOOT_PERSISTENCE")
        second = "second-" + token
        guest.command(f"echo {second} > durable.txt", "> ")
        guest.command("sync", "SYNC_OK")
        guest.close(); guest = None
        result["disk_sha256_after_save"] = hashlib.sha256(disk.read_bytes()).hexdigest()
        guest = Guest(source, output / "cold", disk, args.accel)
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
