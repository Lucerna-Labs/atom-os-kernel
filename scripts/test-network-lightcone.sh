#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
export PATH="$HOME/.cargo/bin:$PATH"
bash "$root/scripts/test-native.sh"
(cd /tmp && cargo +"$toolchain" test --manifest-path "$root/Cargo.toml" --target-dir "$root/target/lightcone-host" --target x86_64-unknown-linux-gnu -p kernel-lightcone -p kernel-net --features std --lib -- --test-threads=1)
