#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
out="$root/target/lightcone-tools"
mkdir -p "$out"
rustc +"${ATOM_TOOLCHAIN:-nightly-2026-08-18}" --edition=2021 --cfg 'feature="std"' --crate-name kernel_lightcone --crate-type rlib \
  "$root/kernel-lightcone/src/lib.rs" -o "$out/libkernel_lightcone.rlib"
rustc +"${ATOM_TOOLCHAIN:-nightly-2026-08-18}" --edition=2021 "$root/tools/lightcone-calibrate.rs" \
  --extern "kernel_lightcone=$out/libkernel_lightcone.rlib" -o "$out/lightcone-calibrate"
"$out/lightcone-calibrate" "$@"
