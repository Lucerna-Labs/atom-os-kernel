#!/usr/bin/env python3
"""Interactive Atom OS serial console, retaining the same data image on exit."""
import argparse
import fcntl
import os
from pathlib import Path
import stat
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="kvm")
    parser.add_argument("--disk", type=Path, default=Path.home() / ".local/share/atom-os-kernel/data.img")
    parser.add_argument("--pidfile", type=Path, help="optional QEMU PID file for supervised sessions")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    image = root / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
    if not image.is_file(): parser.error("build the OS first with bash scripts/build.sh")
    disk = args.disk.absolute()
    disk.parent.mkdir(parents=True, exist_ok=True)
    lock = open(str(disk) + ".lock", "a+")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        parser.error("another Atom OS session is already using this data image")
    try:
        descriptor = os.open(disk, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    except FileExistsError:
        descriptor = os.open(disk, os.O_RDWR | os.O_NOFOLLOW)
    else:
        os.ftruncate(descriptor, 8 * 1024 * 1024)
        os.fsync(descriptor)
    info = os.fstat(descriptor)
    os.close(descriptor)
    if not stat.S_ISREG(info.st_mode) or info.st_size < 2050 * 512:
        parser.error("the data image must be a regular file with at least 2050 sectors")
    # JSON block options preserve paths containing spaces and commas.
    import json
    command = ["qemu-system-x86_64", "-accel", args.accel, "-m", "128M", "-smp", "1",
               "-drive", f"format=raw,file={image},snapshot=on",
               "-blockdev", json.dumps({"driver": "file", "node-name": "atomfile", "filename": str(disk)}),
               "-blockdev", json.dumps({"driver": "raw", "node-name": "atomdata", "file": "atomfile"}),
               "-device", "virtio-blk-pci,drive=atomdata,disable-modern=on,write-cache=on",
               "-display", "none", "-serial", "mon:stdio",
               "-netdev", "user,id=atomnet", "-device", "virtio-net-pci,netdev=atomnet,disable-modern=on"]
    if args.pidfile: command += ["-pidfile", str(args.pidfile.absolute())]
    if args.accel == "kvm" and not os.access("/dev/kvm", os.R_OK | os.W_OK):
        command = ["sudo", "-n", "setpriv", f"--reuid={os.getuid()}", "--regid=kvm", "--init-groups"] + command
    print(f"Atom OS data image: {disk}", flush=True)
    print("Type help in the OS. Use sync to save; Ctrl-a then x exits QEMU. The data image is retained.", flush=True)
    try:
        return subprocess.call(command)
    finally:
        lock.close()


if __name__ == "__main__":
    raise SystemExit(main())
