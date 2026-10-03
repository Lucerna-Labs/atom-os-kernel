#!/usr/bin/env bash
# Refresh the copied pmre-kit from the Atom Rendering Engine. Copies files; creates no
# pins, path references or symlinks to the engine checkout.
#   bash scripts/update-engine.sh [engine-git-url]
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
url=${1:-https://github.com/Lucerna-Labs/atom-rendering-engine.git}
dest="$root/third_party/atom-rendering-engine"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git clone --quiet "$url" "$work/engine"
commit=$(git -C "$work/engine" rev-parse HEAD)
# Apply the local patches in order, skipping any the engine already contains.
for patch in "$dest"/patches/*.patch; do
  subject=$(git mailinfo /dev/null /dev/null <"$patch" | sed -n 's/^Subject: //p')
  if git -C "$work/engine" log --format=%s | grep -Fxq "$subject" ||
     git -C "$work/engine" apply --check --reverse "$patch" 2>/dev/null; then
    echo "already in engine: $(basename "$patch")"
  else
    git -C "$work/engine" -c user.name=atom -c user.email=atom@localhost am --quiet "$patch"
  fi
done
rm -rf "$dest/pmre-kit"
cp -R "$work/engine/pmre-kit" "$dest/pmre-kit"
rm -rf "$dest/pmre-kit/target"
cp "$work/engine/LICENSE" "$work/engine/LICENSE-HISTORY.md" "$dest/"
sed -i "s/^\`[0-9a-f]\{40\}\`/\`$commit\`/" "$dest/UPSTREAM.md"
echo "pmre-kit copied from $commit; rebuild with: bash scripts/build.sh"
