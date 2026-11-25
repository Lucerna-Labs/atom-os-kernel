#!/usr/bin/env python3
"""Atom OS hardware-usage and speed benchmark tracker."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def parse_kv(line):
    return {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", line)}


def read_proc_usage(pid):
    stat_text = Path(f"/proc/{pid}/stat").read_text()
    rparen = stat_text.rfind(")")
    fields = stat_text[rparen + 2:].split()
    cpu_jiffies = int(fields[11]) + int(fields[12])
    rss_kib = 0
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith("VmRSS:"):
            rss_kib = int(line.split()[1])
            break
    return cpu_jiffies, rss_kib


def git_head(source):
    try:
        return subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "--short", "HEAD"],
            text=True, stderr=subprocess.DEVNULL,
        ).strip()
    except Exception:
        return "unknown"


def flatten_metrics(report):
    flat = {
        "boot_to_shell_ms": report["boot_to_shell_ms"],
        "desktop_ready_ms": report["desktop"]["ready_ms"],
        "desktop_frame1_us": report["desktop"]["frame1_us"],
        "host_idle_cpu_pct": report["desktop"]["host_idle_cpu_pct"],
        "host_idle_rss_mib": round(report["desktop"]["host_idle_rss_kib"] / 1024, 1),
    }
    flat.update(report["bench"])
    flat.update({
        "kernel_heap_used_kib": report["mem"]["heap_used_kib"],
        "slab_hit_pct": report["mem"]["slab_hit_pct"],
        "kernel_busy_ticks": report["cpu"]["busy_ticks"],
        "kernel_idle_ticks": report["cpu"]["idle_ticks"],
    })
    return flat


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="kvm")
    parser.add_argument("--cpu", default=None, help="QEMU -cpu for Phase 1 (default: host under kvm)")
    parser.add_argument("--memory", default="8G")
    parser.add_argument("--idle-seconds", type=float, default=5.0)
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--baseline", type=Path, default=None)
    args = parser.parse_args()

    source = args.source.resolve()
    output = args.output.resolve() if args.output else (source / "target/perf-latest.json")
    cpu = args.cpu if args.cpu is not None else ("host" if args.accel == "kvm" else None)
    image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    if not image.exists():
        raise SystemExit(f"Missing bootimage: {image} (run bash scripts/build.sh first)")

    boot_mod = load_module("atom_boot_test", source / "scripts/boot-test.py")
    desktop_mod = load_module("atom_desktop_test", source / "scripts/desktop-test.py")

    report = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "git_head": git_head(source),
        "image_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
        "accel": args.accel,
        "cpu": cpu,
        "memory": args.memory,
    }

    with tempfile.TemporaryDirectory(prefix="atom-perf-") as tmp:
        tmp_path = Path(tmp)

        # Phase 1: kernel boot + shell microbenchmarks
        disk1 = tmp_path / "perf-boot.img"
        with disk1.open("xb") as f:
            f.truncate(64 << 20)
        t0 = time.monotonic()
        guest = boot_mod.Guest(source, tmp_path / "boot", disk1, args.accel, args.memory, cpu=cpu)
        try:
            guest.ready()
            report["boot_to_shell_ms"] = round((time.monotonic() - t0) * 1000, 1)
            cp = guest.wait(r"CPU_PROTECT wp=(\d) nxe=(\d) smep=(\d) smap=(\d)")
            report["cpu_protect"] = {
                "wp": int(cp[1]), "nxe": int(cp[2]), "smep": int(cp[3]), "smap": int(cp[4]),
            }
            offset = len(guest.serial())
            guest.keys("perf\n")
            guest.wait("PERF_OK", offset, seconds=90)
            block = guest.serial()[offset:]
            for key, prefix in [("cpu", "PERF_CPU"), ("mem", "PERF_MEM"), ("io", "PERF_IO"), ("bench", "PERF_BENCH")]:
                match = re.search(rf"^{prefix} .+$", block, re.M)
                if not match:
                    raise RuntimeError(f"Missing {prefix} in output:\n{block}")
                report[key] = parse_kv(match.group(0))
        finally:
            guest.close()

        # Phase 2: graphical desktop startup, frame speed, and idle host footprint
        disk2 = tmp_path / "perf-desktop.img"
        with disk2.open("xb") as f:
            f.truncate(64 << 20)
        dt0 = time.monotonic()
        vm = desktop_mod.Desktop(source, tmp_path / "desktop", disk2, args.accel, args.memory, (1920, 1080))
        try:
            ready = vm.wait(r"DESKTOP_READY (\d+)x(\d+) scale=(\d+)")
            ready_ms = round((time.monotonic() - dt0) * 1000, 1)
            frame1 = vm.wait(r"FRAME 1 render_ms=(\d+) render_us=(\d+)")
            clk_tck = os.sysconf("SC_CLK_TCK")
            pid = vm.process.pid
            j0, rss0 = read_proc_usage(pid)
            w0 = time.monotonic()
            rss_peak = rss0
            samples = max(1, int(args.idle_seconds / 0.5))
            for _ in range(samples):
                time.sleep(args.idle_seconds / samples)
                _, rss = read_proc_usage(pid)
                rss_peak = max(rss_peak, rss)
            w1 = time.monotonic()
            j1, rss1 = read_proc_usage(pid)
            vm.wait(r"DESKTOP_READY")  # Ensure no kernel/user faults occurred during idle window.
            host_cpu_pct = round(((j1 - j0) / clk_tck) / max(w1 - w0, 0.001) * 100.0, 2)
            report["desktop"] = {
                "width": int(ready[1]),
                "height": int(ready[2]),
                "scale": int(ready[3]),
                "ready_ms": ready_ms,
                "frame1_ms": int(frame1[1]),
                "frame1_us": int(frame1[2]),
                "idle_sample_seconds": round(w1 - w0, 2),
                "host_idle_cpu_pct": host_cpu_pct,
                "host_idle_rss_kib": rss1,
                "host_peak_rss_kib": rss_peak,
            }
        finally:
            vm.close()

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n")

    current = flatten_metrics(report)
    baseline = None
    if args.baseline and args.baseline.exists():
        baseline = flatten_metrics(json.loads(args.baseline.read_text()))

    print(f"\nAtom OS Performance Scorecard ({report['git_head']} · {args.accel}/{cpu or 'default'}):")
    for k, v in current.items():
        if baseline and k in baseline and baseline[k]:
            delta = (v - baseline[k]) / baseline[k] * 100.0
            print(f"  {k:24s} {v:>12}   ({delta:+.1f}% vs baseline {baseline[k]})")
        else:
            print(f"  {k:24s} {v:>12}")
    print(f"Saved: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
