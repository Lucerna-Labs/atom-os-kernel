#!/usr/bin/env bash
# Open the Atom OS in a window on your desktop (VGA text console).
# The demo fleet runs, the shell prompt is yours; typing goes through
# the PS/2 keyboard driver, output lands on the VGA console.
# Close the window (or Ctrl-a x in the serial-less case) to exit.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
image="$root/target/x86_64-os/release/bootimage-x86_64-kernel.bin"
if [ ! -f "$image" ]; then
  echo "No boot image — build first: bash scripts/build.sh" >&2
  exit 1
fi
accel=tcg
if [ -w /dev/kvm ]; then accel=kvm; fi
echo "Atom OS opening in a window ($accel). Type 'help' at the prompt; 'ping' speaks to the wire."
exec qemu-system-x86_64 \
  -accel "$accel" -m 128M -smp 1 \
  -drive "format=raw,file=$image,snapshot=on" \
  -netdev user,id=atomnet -device virtio-net-pci,netdev=atomnet,disable-modern=on \
  -vga std
