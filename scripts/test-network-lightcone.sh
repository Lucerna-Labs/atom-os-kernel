#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
out="$root/target/network-lightcone-tests"
mkdir -p "$out"
bash "$root/scripts/test-native.sh"
rustc +"$toolchain" --edition=2021 --test --cfg 'feature="std"' "$root/kernel-lightcone/src/lib.rs" -o "$out/geometry-tests"
"$out/geometry-tests" --test-threads=1
rustc +"$toolchain" --edition=2021 --test "$root/kernel-net/src/lib.rs" -L "dependency=$root/target/native-tests" \
  --extern "kernel_kit=$root/target/native-tests/libkernel_kit.rlib" \
  --extern "kernel_egress=$root/target/native-tests/libkernel_egress.rlib" \
  --extern "kernel_taint=$root/target/native-tests/libkernel_taint.rlib" \
  --extern "kernel_lightcone=$root/target/native-tests/libkernel_lightcone.rlib" -o "$out/network-tap-tests"
"$out/network-tap-tests" --test-threads=1
