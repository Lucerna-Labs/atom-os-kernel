#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
offline=()
if [ "${ATOM_OFFLINE:-0}" = 1 ]; then offline=(--offline); fi
for program in payload daemon worker desktop; do
  (cd "$root/$program" && cargo +"$toolchain" build -Zjson-target-spec --release --locked "${offline[@]}")
done
(cd "$root/x86_64-kernel" && cargo +"$toolchain" bootimage -Zjson-target-spec --release --locked "${offline[@]}")
