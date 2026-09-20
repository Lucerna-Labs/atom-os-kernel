#!/usr/bin/env bash
# Verify additions in the existing Atom VM; fresh build and isolated data disks.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stamp=$(date -u +%Y%m%dT%H%M%S%NZ)
remote="/home/jesse/atom-os-kernel-tests/userspace-$stamp"
results="$root/test-results/userspace-$stamp"
mkdir -p "$results"
if [ "$(virsh --connect qemu:///system domstate atom-os-dev)" = 'shut off' ]; then
  virsh --connect qemu:///system start atom-os-dev
fi
ready=0
for attempt in {1..30}; do
  if ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=2 atom-os-dev true 2>/dev/null; then ready=1; break; fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then echo "Atom VM SSH is unavailable" >&2; exit 1; fi
ssh -o BatchMode=yes atom-os-dev "mkdir -p '$remote/source' '$remote/results'"
rsync -a --exclude=.git --exclude=target --exclude=test-results --exclude=__pycache__ \
  --exclude=.zcode --exclude=.zcode-memory --exclude='*.log' --exclude=_build_err.txt \
  "$root/" "atom-os-dev:$remote/source/"
printf '%s\n' "$remote" > "$results/vm-path.txt"
set +e
ssh -o BatchMode=yes atom-os-dev "bash -s -- '$remote'" <<'REMOTE'
set -euo pipefail
run=$1
cd "$run/source"
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version --verbose > "$run/results/toolchain.txt"
python3 - "$run/results/source-sha256.json" <<'HASHES'
from pathlib import Path
import hashlib,json,sys
root=Path.cwd()
files={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()
       for p in sorted(root.rglob('*')) if p.is_file() and 'target' not in p.relative_to(root).parts}
Path(sys.argv[1]).write_text(json.dumps(files,indent=2)+'\n')
HASHES
bash scripts/build.sh > "$run/results/build.log" 2>&1 || { tail -n 70 "$run/results/build.log"; exit 1; }
printf 'BUILD_AND_ELF_PREFLIGHT_PASS\n'
bash scripts/test-userspace-host.sh > "$run/results/userspace-host.log" 2>&1 || { cat "$run/results/userspace-host.log"; exit 1; }
cat "$run/results/userspace-host.log"
cp target/x86_64-os/release/bootimage-x86_64-kernel.bin "$run/results/bootimage.bin"
cp target/userspace-host-tests/elf-cases/result.json "$run/results/elf-preflight.json"
# Preserve any inherited native failure without hiding the focused VM results.
set +e
bash scripts/test-native.sh > "$run/results/native.log" 2>&1
native=$?
set -e
cp target/native-tests/compile.log "$run/results/native-compile.log"
printf '%s\n' "$native" > "$run/results/native-status.txt"
tail -n 8 "$run/results/native.log"
python3 scripts/test-userspace-imports.py --accel kvm --output "$run/results/kvm"
python3 scripts/test-userspace-imports.py --accel tcg --output "$run/results/tcg"
exit "$native"
REMOTE
status=$?
set -e
rsync -a "atom-os-dev:$remote/results/" "$results/"
printf '%s\n' "$status" > "$results/exit-status.txt"
printf 'Saved results: %s\n' "$results"
exit "$status"
