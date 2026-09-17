#!/usr/bin/env bash
# Build the current working tree in atom-os-dev and attach its serial console.
set -euo pipefail
if [ "${1:-}" = '--help' ]; then
  echo 'Usage: bash scripts/vm-run.sh [--accel kvm|tcg] [--disk /path/inside/vm/data.img]'
  echo 'Each build is isolated; the default data image is reused across sessions.'
  exit 0
fi
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
vm=atom-os-dev
stamp=$(date -u +%Y%m%dT%H%M%S%NZ)
remote="/home/jesse/atom-os-kernel-tests/session-$stamp"
state=$(virsh --connect qemu:///system domstate "$vm")
if [ "$state" = 'shut off' ]; then virsh --connect qemu:///system start "$vm"; fi
ready=0
for attempt in {1..30}; do
  if ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=2 "$vm" true 2>/dev/null; then ready=1; break; fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then echo 'Cannot reach atom-os-dev' >&2; exit 1; fi
ssh -o BatchMode=yes "$vm" "mkdir -p '$remote/source'"
rsync -a --exclude=.git --exclude=target --exclude=test-results --exclude=__pycache__ \
  --exclude=.zcode --exclude=.zcode-memory --exclude='*.log' --exclude=_build_err.txt \
  "$root/" "$vm:$remote/source/"
ssh -o BatchMode=yes "$vm" "cd '$remote/source' && bash scripts/build.sh > '../build.log' 2>&1 || { tail -n 80 '../build.log'; exit 1; }"
remote_command=$(python3 - "$remote" "$@" <<'PY'
import shlex,sys
print(shlex.join(['python3',sys.argv[1]+'/source/scripts/run.py',*sys.argv[2:]]))
PY
)
echo "Build saved in $remote; opening the Atom OS console."
exec ssh -tt -o BatchMode=yes "$vm" "$remote_command"
