#!/usr/bin/env bash
# Open Atom OS in a QEMU window: it boots into the desktop. Files saved with
# "Save all to disk" persist in the data disk between runs.
#   bash scripts/run-desktop.sh [data-disk-path]   (default: target/atom-data.img)
# The virtual monitor reports ATOM_RESOLUTION (default 1920x1080) as its preferred mode,
# and the desktop adapts to it: e.g. ATOM_RESOLUTION=3840x2160 or 6144x3456 (6K).
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
image="$root/target/x86_64-os/release/bootimage-x86_64-kernel.bin"
disk=${1:-"$root/target/atom-data.img"}
if [ ! -f "$image" ]; then echo "No boot image; build first: bash scripts/build.sh" >&2; exit 1; fi
if [ ! -f "$disk" ]; then truncate -s 8M "$disk"; fi
resolution=${ATOM_RESOLUTION:-1920x1080}
xres=${resolution%x*}; yres=${resolution#*x}
# Video memory: the smallest power of two (MiB, at least 16) that holds one 32-bit frame.
vgamem=16
while [ $((vgamem * 1024 * 1024)) -lt $((xres * yres * 4)) ]; do vgamem=$((vgamem * 2)); done
accel=tcg
if [ -w /dev/kvm ]; then accel=kvm; fi
exec qemu-system-x86_64 -accel "$accel" -m "${ATOM_MEMORY:-8G}" -smp 1 \
  -drive "format=raw,file=$image,snapshot=on" \
  -drive "if=none,format=raw,file=$disk,id=atomdata,cache=writeback" \
  -device virtio-blk-pci,drive=atomdata,disable-modern=on \
  -device "VGA,xres=$xres,yres=$yres,vgamem_mb=$vgamem" -serial stdio
