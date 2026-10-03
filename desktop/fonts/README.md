# Desktop fonts

Subsets of DejaVu Sans, DejaVu Sans Bold and DejaVu Sans Mono (Bitstream Vera derived;
see `LICENSE-DejaVu.txt`), cut to printable ASCII plus `· • … — × °` so they fit in the
desktop program image. pmre-kit's TrueType parser reads and rasterizes them at run time.

Regenerate with fontTools:

```sh
for f in DejaVuSans DejaVuSans-Bold DejaVuSansMono; do
  pyftsubset /usr/share/fonts/truetype/dejavu/$f.ttf \
    --unicodes="U+0020-007E,U+00B7,U+2022,U+2026,U+2014,U+00D7,U+00B0" \
    --no-hinting --layout-features='' --drop-tables+=GPOS,GSUB,GDEF,kern,FFTM,DSIG \
    --name-IDs='*' --output-file=desktop/fonts/$f.ttf
done
```
