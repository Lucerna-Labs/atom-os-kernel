#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
out=${ATOM_TEST_OUT:-"$root/target/native-tests"}
mkdir -p "$out"
deps=()
: > "$out/compile.log"
for crate in kernel-kit kernel-net; do
  name=${crate//-/_}
  rustc +"$toolchain" --edition=2024 --crate-name "$name" --crate-type rlib \
    "$root/$crate/src/lib.rs" -L "dependency=$out" "${deps[@]}" \
    -o "$out/lib$name.rlib" 2>> "$out/compile.log"
  deps+=(--extern "$name=$out/lib$name.rlib")
  # Each crate's own unit tests.
  if grep -q '#\[test\]' "$root/$crate/src/lib.rs"; then
    rustc +"$toolchain" --edition=2024 --test --cfg 'feature="std"' \
      "$root/$crate/src/lib.rs" -L "dependency=$out" "${deps[@]}" \
      -o "$out/$name-tests" 2>> "$out/compile.log"
    "$out/$name-tests" --test-threads=1
  fi
done
rustc +"$toolchain" --edition=2021 --test "$root/tests/core.rs" \
  -L "dependency=$out" "${deps[@]}" -o "$out/core-tests" 2>> "$out/compile.log"
"$out/core-tests" --test-threads=1 --nocapture
# The window vocabulary shared by windowed programs and the desktop.
rustc +"$toolchain" --edition=2024 --test "$root/ui-intent/src/lib.rs" -o "$out/ui-intent-tests" 2>> "$out/compile.log"
"$out/ui-intent-tests" --test-threads=1
