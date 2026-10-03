# Atom Rendering Engine (`pmre-kit`, copied in)

The desktop renders through **pmre-kit**, the primitive kit of the
[Atom Rendering Engine](https://github.com/Lucerna-Labs/atom-rendering-engine)
by Jesse G. Alicea / Lucerna Labs, licensed under the PolyForm Small Business License
1.0.0 (see `LICENSE` and `LICENSE-HISTORY.md` here).

This is a **self-contained copy**, not a reference: no git pin, no path to another
checkout, no symlink. Moving or renaming the engine project never breaks the Atom OS
build. The copy is only refreshed when you choose to.

Two local changes live in `patches/`, written for the engine repository too:

- `0001` adds an opt-out `std` feature so the kit builds without the standard library
  (Atom OS programs have none).
- `0002` makes the kit faster and leaner for an interactive desktop: a span fast path
  (interior rows of rects, rounded rects and circles are filled as runs instead of
  evaluating the SDF per pixel; `Surface::fill_span` lets a target bulk-fill them),
  SDF outline shapes (`RoundedRectOutline`, `CircleOutline`) instead of stroked paths,
  and opt-out `uxi`/`html` features so a program that only draws primitives does not
  compile the widget layer or the HTML pipeline.

## Refreshing from the engine

```sh
bash scripts/update-engine.sh            # latest engine main from GitHub
bash scripts/update-engine.sh <git-url>  # or another clone/URL you name
```

The script copies the current `pmre-kit` into this directory and applies each patch
the engine does not already contain. Last copied from engine commit
`fd3f8c273a5150bc0f72ac3d9764ff668d06a32e` plus patches `0001`–`0002`.
