#!/usr/bin/env bash
# Open Atom OS in a QEMU window: it boots into the desktop. Files saved with
# "Save all to disk" persist in the data disk between runs.
#   bash scripts/run-desktop.sh [data-disk-path]   (default: target/atom-data.img)
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
image="$root/target/x86_64-os/release/bootimage-x86_64-kernel.bin"
disk=${1:-"$root/target/atom-data.img"}
if [ ! -f "$image" ]; then echo "No boot image; build first: bash scripts/build.sh" >&2; exit 1; fi
if [ ! -f "$disk" ]; then truncate -s 8M "$disk"; fi
accel=tcg
if [ -w /dev/kvm ]; then accel=kvm; fi
exec qemu-system-x86_64 -accel "$accel" -m "${ATOM_MEMORY:-8G}" -smp 1 \
  -drive "format=raw,file=$image,snapshot=on" \
  -drive "if=none,format=raw,file=$disk,id=atomdata,cache=writeback" \
  -device virtio-blk-pci,drive=atomdata,disable-modern=on \
  -vga std -serial stdio
