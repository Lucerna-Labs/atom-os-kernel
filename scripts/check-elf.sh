#!/usr/bin/env bash
# Validate explicit ELFs, or every executable embedded by this kernel.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${ATOM_TOOLCHAIN:-nightly-2026-08-18}
out=${ATOM_HOST_TOOLS:-"$root/target/host-tools"}
mkdir -p "$out"
rustc +"$toolchain" --edition=2024 "$root/scripts/check-elf.rs" -o "$out/check-elf"
if [ "$#" -eq 0 ]; then
  mapfile -t programs < <(python3 - "$root" <<'EMBEDDED'
from pathlib import Path
import re,sys
root=Path(sys.argv[1])
source=(root/'x86_64-kernel/src/main.rs').read_text()
for name in dict.fromkeys(re.findall(r'include_bytes!\("\.\./\.\./target/x86_64-os/release/([^"/]+)"\)', source)):
    print(root/'target/x86_64-os/release'/name)
EMBEDDED
)
  if [ "${#programs[@]}" -eq 0 ]; then
    echo "No embedded executables discovered; refusing an empty preflight" >&2; exit 1
  fi
else
  programs=("$@")
fi
"$out/check-elf" "${programs[@]}"
