#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
out=${ATOM_TEST_OUT:-"$root/target/native-tests"}
mkdir -p "$out"
# The source-included scheduler now depends on the security/network crates.
# Compile their real implementations in dependency order; no stand-in modules.
deps=()
: > "$out/compile.log"
for crate in kernel-kit kernel-sense kernel-key kernel-instant kernel-egress kernel-lane kernel-taint kernel-crypt kernel-lightcone kernel-net; do
  name=${crate//-/_}
  rustc +"$toolchain" --edition=2024 --crate-name "$name" --crate-type rlib \
    "$root/$crate/src/lib.rs" -L "dependency=$out" "${deps[@]}" \
    -o "$out/lib$name.rlib" 2>> "$out/compile.log"
  deps+=(--extern "$name=$out/lib$name.rlib")
done
rustc +"$toolchain" --edition=2021 --test "$root/tests/core.rs" \
  -L "dependency=$out" "${deps[@]}" -o "$out/core-tests" 2>> "$out/compile.log"
"$out/core-tests" --test-threads=1 --nocapture
