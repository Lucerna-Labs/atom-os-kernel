# Atom OS backup checkpoint

Prepared on September 17, 2026 from `codex/kernel-vm-hardening` for the
Rekonquest GitHub account. This checkpoint contains the complete local kernel
source, userspace runtime and programs, build/test tooling, documentation and
selected historical verification evidence. Preparing this checkpoint does not
by itself confirm that it has been uploaded to GitHub.

The 65 files in the final September 5 tested snapshot were checked again and
still match their recorded SHA-256 values. No new build or test pass is claimed
for September 17; this is a backup of that verified state.

## Recorded verification

- 21 native regression checks passed.
- 16 KVM and 16 TCG acceptance checks passed on the same boot image.
- In both backends, 48 process lifecycles returned free frames from 16,326 to
  16,326, with the shell and daemon remaining alive.
- Exact file contents survived a warm OS reboot and a fresh QEMU cold boot.

The machine-readable result is
[verification.json](test-results/run-20260905T221621361972313Z/verification.json).
The [implementation report](test-results/implementation-20260905/REPORT.md)
and [original failed baseline](test-results/20260905T202917Z/REPORT.md) are also
included. Historical absolute paths in those reports describe the original
local test environment; their selected evidence files are retained under the
same repository-relative paths.

## Preservation boundaries

Existing Git history and tracked files are retained, including prior diagnostic
logs. The small `_build_err.txt` file is retained as historical diagnostic data.
Ignored compiler caches, generated boot/data-disk images, large trace archives
and local agent caches are left out of the source checkpoint. They remain on
the development computer; this is not a backup of the development VM disk.

The inherited `atom-3d-engine` entry is an empty Git link to commit
`48c4e7e843112853b2b45961c33a63dd73d40ecd`, without a `.gitmodules` file.
It is preserved as found. Its absent source is not a dependency of the tested
kernel workspace and cannot be included in this checkpoint.

See [README.md](README.md) for the pinned toolchain, supported limits, and the
repeatable build and VM verification commands.
