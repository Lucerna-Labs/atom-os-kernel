#!/usr/bin/env bash
# Actual kernel image, actual virtio NIC, isolated Ethernet and scratch disks.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stamp=$(date -u +%Y%m%dT%H%M%S%NZ)
remote="/home/jesse/atom-os-kernel-tests/network-world-$stamp"
results="$root/test-results/network-world-$stamp"
mkdir -p "$results"
if [ "$(virsh --connect qemu:///system domstate atom-os-dev)" = 'shut off' ]; then virsh --connect qemu:///system start atom-os-dev; fi
ssh -o BatchMode=yes -o ConnectTimeout=5 atom-os-dev "mkdir -p '$remote/source' '$remote/results'"
rsync -a --exclude=.git --exclude=target --exclude=test-results --exclude=__pycache__ --exclude=.zcode --exclude=.zcode-memory --exclude='*.log' "$root/" "atom-os-dev:$remote/source/"
printf '%s\n' "$remote" > "$results/vm-path.txt"
set +e
ssh -o BatchMode=yes atom-os-dev "bash -s -- '$remote' '${ATOM_PRE_TPU:-0}'" <<'REMOTE'
set -euo pipefail
run=$1
pre_tpu=$2
cd "$run/source"
export PATH="$HOME/.cargo/bin:$PATH"
python3 - "$run/results/source-sha256.json" <<'HASHES'
from pathlib import Path
import hashlib,json,sys
root=Path.cwd()
files={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(root.rglob('*')) if p.is_file() and 'target' not in p.relative_to(root).parts}
Path(sys.argv[1]).write_text(json.dumps(files,indent=2)+'\n')
HASHES
rustc --version --verbose > "$run/results/toolchain.txt"
bash scripts/build.sh > "$run/results/build.log" 2>&1 || { tail -n 70 "$run/results/build.log"; exit 1; }
printf 'BUILD_AND_ELF_PREFLIGHT_PASS\n'
bash scripts/test-network-lightcone.sh > "$run/results/native.log" 2>&1 || { tail -n 50 "$run/results/native.log"; exit 1; }
tail -n 22 "$run/results/native.log"
cp target/x86_64-os/release/bootimage-x86_64-kernel.bin "$run/results/bootimage.bin"
python3 scripts/test-network-lightcone.py --accel kvm --output "$run/results/kvm"
python3 scripts/test-network-lightcone.py --accel tcg --output "$run/results/tcg"
if [ "$pre_tpu" = 1 ]; then
  failed=0
  for acceleration in kvm tcg; do
    python3 scripts/test-network-lightcone-pre-tpu.py --accel "$acceleration" --output "$run/results/pre-tpu-$acceleration" || failed=1
  done
  exit "$failed"
fi
REMOTE
status=$?
set -e
rsync -a "atom-os-dev:$remote/results/" "$results/"
printf '%s\n' "$status" > "$results/exit-status.txt"
printf '%s\n' "$results" > "$root/test-results/latest-network-world.txt"
printf 'Saved results: %s\n' "$results"
exit "$status"
