#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
out="$root/target/c1-exp"
mkdir -p "$out"

rustc +"$toolchain" --edition=2024 -O --crate-name atom_web_fp --crate-type rlib \
  "$root/atom-web-fp/src/lib.rs" -o "$out/libatom_web_fp.rlib"

rustc +"$toolchain" --edition=2024 -O --crate-name atom_web_engine --crate-type rlib \
  "$root/atom-web-engine/src/lib.rs" -L "dependency=$out" \
  --extern "atom_web_fp=$out/libatom_web_fp.rlib" \
  -o "$out/libatom_web_engine.rlib"

rustc +"$toolchain" --edition=2024 -O \
  "$root/experiments/c1-spiderweb/runner.rs" -L "dependency=$out" \
  --extern "atom_web_fp=$out/libatom_web_fp.rlib" \
  --extern "atom_web_engine=$out/libatom_web_engine.rlib" \
  -o "$out/c1-runner"

"$out/c1-runner"
