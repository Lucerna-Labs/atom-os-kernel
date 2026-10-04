# Parked: running unmodified Linux programs

Started on 2026-10-04 and handed to another coder to finish.

## Goal

Run an unmodified, statically linked Linux program on Atom OS. The program
stays sealed: its bytes are never changed. Atom's core stays untouched too.
Whatever the program expects and Atom lacks is composed over it from the
outside, as a separate crate that hooks in at existing chokepoints. The shadow
web and the egress cone are composed over the dispatcher the same way.

## Where it stands

- A small static test program, built with `gcc -static`, copies onto an Atom
  data disk intact, but Atom refuses to start it. The refusal is the starting
  point: each missing function found becomes one composed layer.
- `scripts/atomfs-pack.py` builds an ATOMFS02 data disk holding files from the
  host, so test programs can be booted with `scripts/run.py --disk`.
- An unfinished local draft may exist in `kernel-linux/`. It is not committed
  and not in the workspace. Finish it or start again.

## To finish

Design and build the composed layer, wire its hooks into the kernel, and add an
acceptance test that boots a disk holding the test program and checks its
output and exit code.
