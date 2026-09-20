#!/usr/bin/env bash
# Focused host gates; no VM, syscall, device or source-tree mutation.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
out=${ATOM_TEST_OUT:-"$root/target/userspace-host-tests"}
mkdir -p "$out"
rustc +"${ATOM_TOOLCHAIN:-nightly-2026-08-18}" --edition=2024 --test \
  "$root/user-rt/src/ipc.rs" -o "$out/ipc-tests"
"$out/ipc-tests" --test-threads=1
python3 "$root/scripts/test-elf-check.py" --checker "$root/target/host-tools/check-elf" \
  --elf "$root/target/x86_64-os/release/payload" --output "$out/elf-cases"
