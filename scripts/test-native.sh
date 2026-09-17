#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
out=${ATOM_TEST_OUT:-"$root/target/native-tests"}
mkdir -p "$out"
rustc +"$toolchain" --edition=2024 --crate-name kernel_kit --crate-type rlib \
  "$root/kernel-kit/src/lib.rs" -o "$out/libkernel_kit.rlib" 2> "$out/compile.log"
rustc +"$toolchain" --edition=2021 --test "$root/tests/core.rs" \
  --extern "kernel_kit=$out/libkernel_kit.rlib" -o "$out/core-tests" 2>> "$out/compile.log"
"$out/core-tests" --test-threads=1 --nocapture
