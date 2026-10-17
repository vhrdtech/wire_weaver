# Logo sources

`logos.py` draws `wire_weaver_logo.svg` and `shrink_wrap_logo.svg` in `docs/assets/`, each with a `-dark` variant
and a 2x PNG next to it (the READMEs on GitHub and crates.io embed the PNGs, the docs site the SVGs).
Run `just logos` (or `uv run docs/assets/src/logos.py`); PNGs need `rsvg-convert` (librsvg).

The output is plain SVG: cubic Béziers, a mask for the clear rim where one conductor crosses over the other, and
for ShrinkWrap a clip path and gradient mask for the pair's relief under the sleeve. No filters, no fonts: the
wordmarks are JetBrains Mono Bold (`fonts/`, SIL Open Font License, see `fonts/JetBrainsMono-OFL.txt`) turned into
outlines with fontTools. It is a Python script rather than Typst because Typst's SVG export has no masks or clip
paths, which the weave and the relief need.

The geometry is the one of the original logos (kept as `*_snake.svg`): conductors 20 wide, 30 apart, crossing every
48 units, highlights 4.4 above the centre line, copper 10 wide with its tip 22 out from the insulation cut. The
constants at the top of `logos.py` name them.
