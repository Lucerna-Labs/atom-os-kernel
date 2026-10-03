# Atom Rendering Engine (`pmre-kit`, copied in)

The desktop renders through **pmre-kit**, the primitive kit of the
[Atom Rendering Engine](https://github.com/Lucerna-Labs/atom-rendering-engine)
by Jesse G. Alicea / Lucerna Labs, licensed under the PolyForm Small Business License
1.0.0 (see `LICENSE` and `LICENSE-HISTORY.md` here).

This is a **self-contained copy**, not a reference: no git pin, no path to another
checkout, no symlink. Moving or renaming the engine project never breaks the Atom OS
build. The copy is only refreshed when you choose to.

The one local change is `patches/0001-*.patch`, which adds an opt-out `std` feature so
the kit builds without the standard library (Atom OS programs have none). It is written
for the engine repository too; once the engine has it, the refresh needs no patch.

## Refreshing from the engine

```sh
bash scripts/update-engine.sh            # latest engine main from GitHub
bash scripts/update-engine.sh <git-url>  # or another clone/URL you name
```

The script copies the current `pmre-kit` into this directory and re-applies the patch
when the engine does not have the `std` feature yet. Last copied from engine commit
`fd3f8c273a5150bc0f72ac3d9764ff668d06a32e`.
