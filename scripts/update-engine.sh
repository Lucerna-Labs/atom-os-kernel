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
git clone --quiet --depth 1 "$url" "$work/engine"
commit=$(git -C "$work/engine" rev-parse HEAD)
if ! grep -q '^std = \[\]' "$work/engine/pmre-kit/Cargo.toml"; then
  git -C "$work/engine" -c user.name=atom -c user.email=atom@localhost am --quiet "$dest"/patches/0001-*.patch
fi
rm -rf "$dest/pmre-kit"
cp -R "$work/engine/pmre-kit" "$dest/pmre-kit"
rm -rf "$dest/pmre-kit/target"
cp "$work/engine/LICENSE" "$work/engine/LICENSE-HISTORY.md" "$dest/"
sed -i "s/^\`[0-9a-f]\{40\}\`\.$/\`$commit\`./" "$dest/UPSTREAM.md"
echo "pmre-kit copied from $commit; rebuild with: bash scripts/build.sh"
