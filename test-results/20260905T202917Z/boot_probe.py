#!/usr/bin/env python3
"""Exercise the unchanged kernel image inside its existing build VM."""
import argparse
import hashlib
import json
import pathlib
import re
import socket
import subprocess
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", type=pathlib.Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], required=True)
    parser.add_argument("--label")
    parser.add_argument("--trace", action="store_true")
    parser.add_argument("--cpu")
    parser.add_argument("--boot-seconds", type=float, default=20)
    args = parser.parse_args()
    run = args.run.resolve()
    output = run / "results" / ("boot-" + (args.label or args.accel))
    output.mkdir(parents=True, exist_ok=False)
    image = run / "source/target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    command = ["qemu-system-x86_64", "-accel", args.accel,
               "-m", "128M", "-smp", "1", "-drive",
               f"format=raw,file={image},snapshot=on", "-display", "none",
               "-serial", f"file:{output / 'serial.log'}", "-nic", "none",
               "-qmp", f"unix:{output / 'qmp.sock'},server=on,wait=off",
               "-no-reboot", "-no-shutdown", "-d", "int,cpu_reset,guest_errors" if args.trace else "guest_errors",
               "-D", str(output / "guest-errors.log")]
    if args.cpu:
        command += ["-cpu", args.cpu]
    if args.accel == "kvm":
        command = ["sudo", "-n", "setpriv", "--reuid=1000", "--regid=kvm", "--init-groups"] + command
    result = {"command": command, "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
              "acceleration": args.accel, "checks": {}, "commands": [], "events": []}
    (output / "command.json").write_text(json.dumps(command, indent=2) + "\n")
    start = time.monotonic()
    stderr = (output / "qemu.stderr.log").open("w")
    proc = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=stderr)
    client = None
    wire = None
    trace_active = args.trace

    def serial():
        path = output / "serial.log"
        return path.read_text(errors="replace").replace("\r", "") if path.exists() else ""

    def qmp(execute, arguments=None):
        request = {"execute": execute}
        if arguments is not None:
            request["arguments"] = arguments
        wire.write(json.dumps(request).encode() + b"\n")
        wire.flush()
        while True:
            line = wire.readline()
            if not line:
                raise RuntimeError("QMP disconnected")
            response = json.loads(line)
            if "event" in response:
                result["events"].append(response)
                continue
            if "error" in response:
                raise RuntimeError(str(response))
            return response["return"]

    def wait_for(predicate, seconds):
        nonlocal trace_active
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline and proc.poll() is None:
            current = serial()
            trace_path = output / "guest-errors.log"
            if trace_active and ("Ring 3 Multi-Tasking Spawned" in current or "CPU EXCEPTION" in current
                                 or (trace_path.exists() and trace_path.stat().st_size > 8 * 1024 * 1024)):
                qmp("human-monitor-command", {"command-line": "log none"})
                trace_active = False
                result["trace_capture_note"] = "Startup trace stopped at ring-3 launch, exception, or 8 MiB budget."
            if predicate(current):
                return True
            time.sleep(0.1)
        return predicate(serial())

    def keys(value):
        mapping = {" ": "spc", "\n": "ret", ".": "dot", ">": "kp_add", "-": "minus"}
        for character in value:
            qmp("send-key", {"keys": [{"type": "qcode", "data": mapping.get(character, character)}],
                             "hold-time": 30})
            time.sleep(0.085)

    def shell(command_text, marker, timeout=5):
        offset = len(serial())
        keys(command_text + "\n")
        passed = wait_for(lambda data: bool(re.search(marker, data[offset:])), timeout)
        chunk = serial()[offset:]
        result["commands"].append({"command": command_text, "expected_regex": marker,
                                   "passed": passed, "serial": chunk})
        return passed

    try:
        socket_path = output / "qmp.sock"
        deadline = time.monotonic() + 10
        while not socket_path.exists() and proc.poll() is None and time.monotonic() < deadline:
            time.sleep(0.1)
        client = socket.socket(socket.AF_UNIX)
        client.settimeout(5)
        client.connect(str(socket_path))
        wire = client.makefile("rwb")
        result["qmp_greeting"] = json.loads(wire.readline())
        qmp("qmp_capabilities")
        result["kvm"] = qmp("query-kvm")
        print(json.dumps({"accel": args.accel, "kvm": result["kvm"]}), flush=True)
        ready = wait_for(lambda data: bool(re.search(r"10,000 SYS_YIELDs took \(CPU cycles\): \d+\n> ", data)), args.boot_seconds)
        boot = serial()
        (output / "boot-serial.log").write_text(boot)
        result["checks"].update({
            "kernel_banner": "Booting Fearless Hypatia" in boot,
            "heap_smoke": "Heap Allocation Test: PASS!" in boot,
            "slab_benchmark_reported": "oom_after_n: GATE PASS" in boot,
            "ring3_spawn_reported": "Ring 3 Multi-Tasking Spawned." in boot,
            "yield_benchmark_and_prompt": ready,
        })
        print(json.dumps({"accel": args.accel, "boot_checks": result["checks"]}), flush=True)
        if ready:
            result["checks"]["list_files"] = shell("ls", r"shell\.elf.*daemon\.elf")
            shell("echo atomtest > probe.txt", r"> ")
            result["checks"]["ramfs_write_read"] = shell("cat probe.txt", r"\natomtest\n")
            result["checks"]["ipc_delivery"] = shell("msg baseline ipc", r"Received IPC: baseline ipc", 15)
            result["checks"]["repeated_yield_benchmark"] = shell("bench", r"10,000 SYS_YIELDs took \(CPU cycles\): \d+\n", 15)
        else:
            result["checks"]["keyboard_command_response"] = shell("ls", r"shell\.elf.*daemon\.elf", 5)
            result["blocked_checks"] = ["ramfs_write_read", "ipc_delivery", "repeated_yield_benchmark"]
            result["blocked_reason"] = "Boot did not reach the benchmark and interactive prompt."
        result["cpu_samples"] = []
        for _ in range(3):
            result["cpu_samples"].append(qmp("human-monitor-command", {"command-line": "info registers"}))
            time.sleep(0.25)
        result["qemu_status"] = qmp("query-status")
        data = serial()
        result["checks"]["daemon_observed"] = "[Daemon]" in data
        result["exceptions"] = [line for line in data.splitlines() if re.search(r"EXCEPTION|PANIC|OUT OF MEMORY|FAULT", line)]
        result["checks"]["no_reported_exception"] = not result["exceptions"]
        if result["exceptions"]:
            qmp("stop")
            halted = qmp("human-monitor-command", {"command-line": "info registers"})
            stack_address = re.search(r"RSP=([0-9a-fA-F]+)", halted).group(1)
            stack = qmp("human-monitor-command", {"command-line": f"x/128gx 0x{stack_address}"})
            (output / "exception-stack.txt").write_text(stack)
        qmp("quit")
        proc.wait(timeout=10)
    except Exception as error:
        result["harness_error"] = repr(error)
        if wire:
            try:
                qmp("quit")
            except Exception:
                pass
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
    finally:
        if wire:
            wire.close()
        if client:
            client.close()
        stderr.close()
        result["elapsed_seconds"] = round(time.monotonic() - start, 3)
        result["qemu_exit"] = proc.returncode
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        if (output / "qmp.sock").exists():
            (output / "qmp.sock").unlink()
        print(json.dumps({k: v for k, v in result.items() if k in ("acceleration", "checks", "exceptions", "harness_error", "elapsed_seconds", "qemu_exit")}), flush=True)
    return int("harness_error" in result or not all(result["checks"].values()))


if __name__ == "__main__":
    raise SystemExit(main())
