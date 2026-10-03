#!/usr/bin/env bash
# Build and exercise this working tree in the existing, dedicated Atom OS VM.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
vm=atom-os-dev
stamp=$(date -u +%Y%m%dT%H%M%S%NZ)
remote="/home/jesse/atom-os-kernel-tests/run-$stamp"
local_results="$root/test-results/run-$stamp"
accel=${ATOM_ACCEL:-kvm}
case "$accel" in kvm|tcg) ;; *) echo "ATOM_ACCEL must be kvm or tcg" >&2; exit 2;; esac
mkdir -p "$local_results"
state=$(virsh --connect qemu:///system domstate "$vm")
if [ "$state" = 'shut off' ]; then virsh --connect qemu:///system start "$vm"; fi
ready=0
for attempt in {1..30}; do
  if ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=2 "$vm" true 2>/dev/null; then ready=1; break; fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then echo "Cannot reach the existing $vm SSH configuration" >&2; exit 1; fi
ssh -o BatchMode=yes "$vm" "mkdir -p '$remote/source' '$remote/results'"
rsync -a --exclude=.git --exclude=target --exclude=test-results --exclude=.zcode \
  --exclude=.zcode-memory --exclude='*.log' --exclude=_build_err.txt \
  "$root/" "$vm:$remote/source/"
printf '%s\n' "$remote" > "$local_results/vm-path.txt"
set +e
ssh -o BatchMode=yes "$vm" "bash -s -- '$remote' '$accel'" <<'SH'
set -euo pipefail
run=$1
accel=$2
cd "$run/source"
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version --verbose > "$run/results/toolchain.txt"
python3 - <<'PY'
import hashlib,json,pathlib
root=pathlib.Path.cwd()
files={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest()
       for p in sorted(root.rglob('*')) if p.is_file() and 'target' not in p.relative_to(root).parts}
(root.parent/'results/source-sha256.json').write_text(json.dumps(files,indent=2)+'\n')
PY
tar -czf "$run/results/source.tar.gz" --exclude=target .
bash scripts/build.sh > "$run/results/build.log" 2>&1 || { tail -n 80 "$run/results/build.log"; exit 1; }
printf 'BUILD PASS\n'
bash scripts/test-native.sh > "$run/results/native.log" 2>&1 || { cat "$run/results/native.log"; cat target/native-tests/compile.log; exit 1; }
cat "$run/results/native.log"
cp target/native-tests/compile.log "$run/results/native-compile.log"
cp target/x86_64-os/release/bootimage-x86_64-kernel.bin "$run/results/bootimage.bin"
python3 scripts/boot-test.py --accel "$accel" --output "$run/results/acceptance"
python3 scripts/desktop-test.py --accel "$accel" --output "$run/results/desktop"
SH
status=$?
set -e
rsync -a "$vm:$remote/results/" "$local_results/"
printf '%s\n' "$status" > "$local_results/exit-status.txt"
printf '%s\n' "$local_results" > "$root/test-results/latest-run.txt"
printf 'Saved results: %s\n' "$local_results"
exit "$status"
