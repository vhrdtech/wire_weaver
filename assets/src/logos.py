# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools>=4.50"]
# ///
"""Generate the WireWeaver and ShrinkWrap logos (light and dark SVG, 2x PNG).

Run from anywhere: `uv run docs/assets/src/logos.py` (or `just logos`). PNGs need `rsvg-convert`.

Everything is plain SVG 1.1 that GitHub and browsers render as is: cubic Béziers, a mask for the
gaps where one conductor crosses over the other, and for ShrinkWrap a clip path plus a gradient
mask for the relief of the pair under the sleeve. The text is JetBrains Mono Bold (OFL, vendored in
`fonts/`) converted to outlines, so the logos need no font.

Units: 1 SVG unit = 1 px of the 1x logo, PNGs are rendered at 2x.
"""

from __future__ import annotations

import math
import subprocess
from dataclasses import dataclass
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

SRC = Path(__file__).resolve().parent
OUT = SRC.parent
FONT = SRC / "fonts" / "JetBrainsMono-Bold.ttf"

# Twisted pair, measured off the original logos: conductor centres 30 apart (amplitude 15), a strand
# crosses the other every 48 units, conductor 20 wide, highlight 4 wide 4.4 above the centre line.
AMP = 15.0
HALF = 48.0
WIRE_W = 20.0
HI_W = 4.0
HI_OFF = 4.4
GAP = 2.4  # clear rim between a conductor and what passes under it
LEAD = 34.0  # straight lead-in between the insulation cut and the first crest
# Copper: 10 wide, round tip, tip centre 22 out from the cut, highlight 2.5 wide 2 above the centre.
CU_W = 10.0
CU_LEN = 22.0
CU_UNDER = 8.0  # how far the copper runs on under the insulation
CU_HI_W = 2.5
CU_HI_OFF = 2.0
CU_HI_STOP = 3.0  # copper highlight ends this far before the cut (its round end stays in the copper)


@dataclass(frozen=True)
class Theme:
    red: str
    red_hi: str
    black: str
    black_hi: str
    cu: str
    cu_hi: str
    text: str
    sleeve: str
    sleeve_rim: str
    sleeve_text: str
    relief_hi: float  # opacity of the white ridge highlight under the sleeve
    relief_sh: float  # opacity of the black ridge shadow under the sleeve


def blend(fg: str, bg: str, a: float) -> str:
    """`fg` at opacity `a` over `bg`, as one opaque colour (the originals drew highlights at 0.85)."""
    f = [int(fg[i : i + 2], 16) for i in (1, 3, 5)]
    b = [int(bg[i : i + 2], 16) for i in (1, 3, 5)]
    return "#" + "".join(f"{round(a * x + (1 - a) * y):02x}" for x, y in zip(f, b))


LIGHT = Theme(
    red="#c8141c",
    red_hi=blend("#f24c44", "#c8141c", 0.85),
    black="#1a1a1a",
    black_hi=blend("#5a5a5a", "#1a1a1a", 0.85),
    cu="#b8732f",
    cu_hi="#e8a868",
    text="#1a1a1a",
    sleeve="#1a1a1a",
    sleeve_rim="#555555",
    sleeve_text="#ffffff",
    relief_hi=0.16,
    relief_sh=0.30,
)
DARK = Theme(
    red="#d81e1e",
    red_hi=blend("#f26a5c", "#d81e1e", 0.85),
    black="#4a4a4c",
    black_hi=blend("#8a8a8c", "#4a4a4c", 0.85),
    cu="#b8732f",
    cu_hi="#e8a868",
    text="#ececec",
    sleeve="#e6e6e6",
    sleeve_rim="#ffffff",
    sleeve_text="#1a1a1a",
    relief_hi=0.55,
    relief_sh=0.10,
)


# ---------------------------------------------------------------------------------------------
# Curves


def fmt(v: float) -> str:
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s == "-0" else s


def pt(p: tuple[float, float]) -> str:
    return f"{fmt(p[0])},{fmt(p[1])}"


def fit_cubic(samples, t0, t1):
    """Cubic through the first and last sample with end tangents t0, t1 (unit vectors, t1 pointing
    along the curve) and handle lengths fitted to the samples by least squares (Schneider)."""
    p0, p3 = samples[0], samples[-1]
    d = [0.0]
    for a, b in zip(samples, samples[1:]):
        d.append(d[-1] + math.dist(a, b))
    us = [x / d[-1] for x in d]
    c11 = c12 = c22 = x1 = x2 = 0.0
    for u, s in zip(us, samples):
        b0, b1, b2, b3 = (1 - u) ** 3, 3 * u * (1 - u) ** 2, 3 * u * u * (1 - u), u**3
        a1 = (t0[0] * b1, t0[1] * b1)
        a2 = (-t1[0] * b2, -t1[1] * b2)
        r = (
            s[0] - (b0 + b1) * p0[0] - (b2 + b3) * p3[0],
            s[1] - (b0 + b1) * p0[1] - (b2 + b3) * p3[1],
        )
        c11 += a1[0] ** 2 + a1[1] ** 2
        c12 += a1[0] * a2[0] + a1[1] * a2[1]
        c22 += a2[0] ** 2 + a2[1] ** 2
        x1 += a1[0] * r[0] + a1[1] * r[1]
        x2 += a2[0] * r[0] + a2[1] * r[1]
    det = c11 * c22 - c12 * c12
    al = (x1 * c22 - x2 * c12) / det
    be = (c11 * x2 - c12 * x1) / det
    return (
        (p0[0] + al * t0[0], p0[1] + al * t0[1]),
        (p3[0] - be * t1[0], p3[1] - be * t1[1]),
        p3,
    )


class Curve:
    """A function y(x) with its slope, drawn as cubics split at the given x values."""

    def __init__(self, y, dy):
        self.y, self.dy = y, dy

    def point(self, x: float, off: float = 0.0):
        """Point on the curve shifted by `off` along the upward normal."""
        s = self.dy(x)
        n = math.hypot(1, s)
        return (x + off * s / n, self.y(x) - off / n)

    def tangent(self, x: float, off: float = 0.0):
        h = 1e-4
        a, b = self.point(x - h, off), self.point(x + h, off)
        n = math.dist(a, b)
        return ((b[0] - a[0]) / n, (b[1] - a[1]) / n)

    def cubics(self, xs: list[float], off: float = 0.0) -> str:
        out = []
        for a, b in zip(xs, xs[1:]):
            samples = [self.point(a + (b - a) * i / 64, off) for i in range(65)]
            c1, c2, p3 = fit_cubic(samples, self.tangent(a, off), self.tangent(b, off))
            out.append(f"C{pt(c1)} {pt(c2)} {pt(p3)}")
        return " ".join(out)

    def path(self, xs: list[float], off: float = 0.0) -> str:
        return f"M{pt(self.point(xs[0], off))} " + self.cubics(xs, off)


@dataclass
class Pair:
    """The twisted pair: insulation from x0 to x1, centre line at cy, `halves` crossings."""

    x0: float
    cy: float
    halves: int

    @property
    def ts(self) -> float:  # first crest, end of the left lead-in
        return self.x0 + LEAD

    @property
    def te(self) -> float:  # last crest
        return self.ts + self.halves * HALF

    @property
    def x1(self) -> float:
        return self.te + LEAD

    def strand(self, sign: int) -> Curve:
        """sign -1: red, starts on top; +1: black, starts at the bottom."""
        ts, cy = self.ts, self.cy

        def y(x):
            if x <= ts or x >= self.te:
                ph = 0.0 if x <= ts else self.halves * math.pi
            else:
                ph = math.pi * (x - ts) / HALF
            return cy + sign * AMP * math.cos(ph)

        def dy(x):
            if x <= ts or x >= self.te:
                return 0.0
            return -sign * AMP * math.pi / HALF * math.sin(math.pi * (x - ts) / HALF)

        return Curve(y, dy)

    def knots(self, a: float, b: float) -> list[float]:
        """Split points (every crest and crossing) between a and b."""
        q = HALF / 2
        xs = [a]
        k = math.floor((a - self.ts) / q) + 1
        while self.ts + k * q < b - 1e-6:
            xs.append(self.ts + k * q)
            k += 1
        xs.append(b)
        return xs

    def full_path(self, sign: int, off: float) -> str:
        c = self.strand(sign)
        p0, p1 = c.point(self.x0, off), c.point(self.x1, off)
        return (
            f"M{pt(p0)} H{fmt(self.ts)} "
            + c.cubics(self.knots(self.ts, self.te), off)
            + f" H{fmt(p1[0])}"
        )

    def front(self, i: int) -> tuple[int, float]:
        """The strand on top in half-period i is the one going down: red in even halves."""
        return (-1 if i % 2 == 0 else 1), self.ts + i * HALF


def stroke(d: str, color: str, width: float, cap: str = "butt", extra: str = "") -> str:
    return (
        f'<path d="{d}" fill="none" stroke="{color}" stroke-width="{fmt(width)}" '
        f'stroke-linecap="{cap}"{extra}/>'
    )


def pair_svg(p: Pair, th: Theme, mask_id: str) -> tuple[str, str]:
    """(defs, body) of the twisted pair with its copper ends."""
    colors = {-1: (th.red, th.red_hi), 1: (th.black, th.black_hi)}
    # Back layer: both strands in full, cut where the front strand of each half-period passes
    # over. The cut stops 3 units short of the crests, where the front piece lies on top of the
    # same strand, so the joint has no seam.
    cut = []
    for i in range(p.halves):
        s, x = p.front(i)
        d = p.strand(s).path(p.knots(x + 3, x + HALF - 3))
        cut.append(stroke(d, "#000", WIRE_W + 2 * GAP))
    defs = (
        f'<mask id="{mask_id}" maskUnits="userSpaceOnUse" x="-100" y="-100" width="1000" height="400">'
        f'<rect x="-100" y="-100" width="1000" height="400" fill="#fff"/>' + "".join(cut) + "</mask>"
    )
    body = []
    # Copper first, so the insulation lies over it: it comes out of the flat cut.
    for s in (-1, 1):
        y = p.strand(s).y(p.x0)
        for x_cut, out in ((p.x0, -1), (p.x1, 1)):
            tip = x_cut + out * CU_LEN
            body.append(stroke(f"M{pt((x_cut - out * CU_UNDER, y))} H{fmt(tip)}", th.cu, CU_W, "round"))
            yh = y - CU_HI_OFF
            body.append(
                stroke(f"M{pt((x_cut + out * CU_HI_STOP, yh))} H{fmt(tip)}", th.cu_hi, CU_HI_W, "round")
            )
    back = []
    for s in (-1, 1):
        back.append(stroke(p.full_path(s, 0), colors[s][0], WIRE_W))
        back.append(stroke(p.full_path(s, HI_OFF), colors[s][1], HI_W))
    body.append(f'<g mask="url(#{mask_id})">' + "".join(back) + "</g>")
    for i in range(p.halves):
        s, x = p.front(i)
        c, xs = p.strand(s), p.knots(x, x + HALF)
        body.append(stroke(c.path(xs), colors[s][0], WIRE_W))
        body.append(stroke(c.path(xs, HI_OFF), colors[s][1], HI_W))
    return defs, "".join(body)


# ---------------------------------------------------------------------------------------------
# Text


@dataclass
class Text:
    d: str
    bbox: tuple[float, float, float, float]  # ink box x0, y0, x1, y1


def text_outline(s: str, size: float, cx: float, cy: float | None = None, baseline: float | None = None) -> Text:
    """`s` as one outline path, ink box centred on cx (and on cy, or sat on `baseline`)."""
    font = TTFont(FONT)
    gs, cmap = font.getGlyphSet(), font.getBestCmap()
    k = size / font["head"].unitsPerEm
    glyphs, adv = [], 0.0
    bx0 = by0 = math.inf
    bx1 = by1 = -math.inf
    for ch in s:
        g = gs[cmap[ord(ch)]]
        bp = BoundsPen(gs)
        g.draw(bp)
        if bp.bounds:
            a, b, c, d = bp.bounds
            bx0, bx1 = min(bx0, adv + a), max(bx1, adv + c)
            by0, by1 = min(by0, b), max(by1, d)
        glyphs.append((g, adv))
        adv += g.width
    ox = cx - k * (bx0 + bx1) / 2
    oy = cy + k * (by0 + by1) / 2 if cy is not None else baseline
    pen = SVGPathPen(gs, ntos=fmt)
    for g, a in glyphs:
        g.draw(TransformPen(pen, (k, 0, 0, -k, ox + k * a, oy)))
    return Text(pen.getCommands(), (ox + k * bx0, oy - k * by1, ox + k * bx1, oy - k * by0))


# ---------------------------------------------------------------------------------------------
# Logos


def svg_doc(view: tuple[float, float, float, float], defs: str, body: str, title: str) -> str:
    x, y, w, h = view
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{fmt(w)}" height="{fmt(h)}" '
        f'viewBox="{fmt(x)} {fmt(y)} {fmt(w)} {fmt(h)}" role="img" aria-label="{title}">'
        f"<title>{title}</title><defs>{defs}</defs>{body}</svg>\n"
    )


def wire_weaver(th: Theme) -> str:
    p = Pair(x0=40, cy=64, halves=8)
    defs, body = pair_svg(p, th, "weave")
    t = text_outline("WireWeaver", 46.1, cx=(p.x0 + p.x1) / 2, baseline=150.3)
    body += f'<path d="{t.d}" fill="{th.text}"/>'
    return svg_doc((-1, 25, 534, 151), defs, body, "WireWeaver")


# Sleeve: a tube between the crossings at 146 and 482 (its round mouths sit on those crossings).
# The wall runs 41..99 at a crest and 44..96 between crests: the gentle wave is the pair pressing
# on it. Towards each end the wave fades out and the tube narrows gently to the mouth, about 12%
# narrower than the middle, then closes in a round end.
SLEEVE_H = 27.5  # mean half height in the middle
SLEEVE_WAVE = 1.5
SLEEVE_MOUTH = 24.0  # half height at the mouth
SLEEVE_SHOULDER = 40.0  # length of the taper from the middle to the mouth
SLEEVE_END = 10.0  # how far the round end reaches past the mouth


def smoothstep(u: float) -> float:
    u = min(max(u, 0.0), 1.0)
    return u * u * (3 - 2 * u)


def dsmoothstep(u: float) -> float:
    return 6 * u * (1 - u) if 0 < u < 1 else 0.0


def sleeve_outline(p: Pair, xa: float, xb: float) -> tuple[str, Curve, float, float]:
    """Closed outline of the sleeve, round ends reaching to xa and xb. Also returns the top wall
    curve and the x range where the wall is at full height."""
    cy, phase = p.cy, p.ts
    ma, mb = xa + SLEEVE_END, xb - SLEEVE_END
    w = 2 * math.pi / HALF

    def blend(x):  # 0 at a mouth, 1 once the shoulder is done
        u = min(x - ma, mb - x) / SLEEVE_SHOULDER
        du = (1 if x - ma < mb - x else -1) / SLEEVE_SHOULDER
        return smoothstep(u), dsmoothstep(u) * du

    def half(x):
        b, _ = blend(x)
        full = SLEEVE_H + SLEEVE_WAVE * math.cos(w * (x - phase))
        return SLEEVE_MOUTH + (full - SLEEVE_MOUTH) * b

    def dhalf(x):
        b, db = blend(x)
        full = SLEEVE_H + SLEEVE_WAVE * math.cos(w * (x - phase))
        dfull = -SLEEVE_WAVE * w * math.sin(w * (x - phase))
        return (full - SLEEVE_MOUTH) * db + dfull * b

    top = Curve(lambda x: cy - half(x), lambda x: -dhalf(x))
    bot = Curve(lambda x: cy + half(x), dhalf)
    n = round((mb - ma) / 6)
    xs = [ma + (mb - ma) * i / n for i in range(n + 1)]
    k = 0.5523  # quarter ellipse handle
    r, h = SLEEVE_END, SLEEVE_MOUTH

    def end(xm, out, down):
        y0, y1 = (cy - h, cy + h) if down else (cy + h, cy - h)
        ax = xm + out * r
        return (
            f"C{pt((xm + out * k * r, y0))} {pt((ax, cy + (y0 - cy) * k))} {pt((ax, cy))} "
            f"C{pt((ax, cy + (y1 - cy) * k))} {pt((xm + out * k * r, y1))} {pt((xm, y1))}"
        )

    d = (
        f"M{pt((ma, cy - h))} "
        + top.cubics(xs)
        + " "
        + end(mb, 1, True)
        + " "
        + bot.cubics(xs[::-1])
        + " "
        + end(ma, -1, False)
        + " Z"
    )
    return d, top, ma + SLEEVE_SHOULDER, mb - SLEEVE_SHOULDER


def relief(p: Pair, th: Theme, xa: float, xb: float) -> tuple[str, str]:
    """(defs, body) of the pair showing through the sleeve: a soft ridge along the front strand
    of every crossing under it, a highlight on its upper edge and a faint shadow on its lower edge.
    Same curve as the visible conductors, so it continues their twist at both mouths. Each ridge
    runs from crest to crest and fades out at its ends."""
    defs, out = [], []
    for i in range(p.halves):
        s, x = p.front(i)
        if x + HALF < xa or x > xb:
            continue
        c = p.strand(s)
        xs = p.knots(x + 1, x + HALF - 1)
        for kind, color, opacity, layers in (
            ("hi", "#fff", th.relief_hi, ((3, 14, 0.25), (4.5, 8, 0.35), (5.5, 3.5, 0.4))),
            ("sh", "#000", th.relief_sh, ((-7, 8, 0.4), (-8.5, 4, 0.6))),
        ):
            gid = f"ridge-{kind}-{i}"
            p0, p1 = c.point(xs[0]), c.point(xs[-1])
            defs.append(
                f'<linearGradient id="{gid}" gradientUnits="userSpaceOnUse" x1="{fmt(p0[0])}" '
                f'y1="{fmt(p0[1])}" x2="{fmt(p1[0])}" y2="{fmt(p1[1])}">'
                f'<stop offset="0" stop-color="{color}" stop-opacity="0"/>'
                f'<stop offset="0.2" stop-color="{color}"/><stop offset="0.8" stop-color="{color}"/>'
                f'<stop offset="1" stop-color="{color}" stop-opacity="0"/></linearGradient>'
            )
            for off, w, a in layers:
                out.append(stroke(c.path(xs, off), f"url(#{gid})", w, "butt", f' opacity="{fmt(opacity * a)}"'))
    return "".join(defs), "".join(out)


def shrink_wrap(th: Theme) -> str:
    p = Pair(x0=40, cy=70, halves=10)
    pdefs, pbody = pair_svg(p, th, "weave")
    xa, xb = p.ts + 1.5 * HALF, p.te - 1.5 * HALF  # crossings at 146 and 482
    outline, top, full0, full1 = sleeve_outline(p, xa, xb)
    t = text_outline("ShrinkWrap", 34.2, cx=(xa + xb) / 2, cy=p.cy)
    rdefs, rbody = relief(p, th, xa, xb)
    n = round((full1 - full0) / 6)
    rim_x = [full0 - 24 + (full1 - full0 + 48) * i / n for i in range(n + 1)]
    # Relief fades out across the text band: full at the walls, faint behind the letters.
    y0, y1 = p.cy - SLEEVE_H - SLEEVE_WAVE, p.cy + SLEEVE_H + SLEEVE_WAVE
    f0 = (t.bbox[1] - y0) / (y1 - y0)
    f1 = (t.bbox[3] - y0) / (y1 - y0)
    defs = (
        pdefs
        + rdefs
        + f'<linearGradient id="rim-fade" gradientUnits="userSpaceOnUse" x1="{fmt(rim_x[0])}" y1="0" x2="{fmt(rim_x[-1])}" y2="0">'
        f'<stop offset="0" stop-color="{th.sleeve_rim}" stop-opacity="0"/>'
        f'<stop offset="0.12" stop-color="{th.sleeve_rim}"/><stop offset="0.88" stop-color="{th.sleeve_rim}"/>'
        f'<stop offset="1" stop-color="{th.sleeve_rim}" stop-opacity="0"/></linearGradient>'
        f'<clipPath id="sleeve-clip"><path d="{outline}"/></clipPath>'
        f'<linearGradient id="relief-fade" gradientUnits="userSpaceOnUse" x1="0" y1="{fmt(y0)}" x2="0" y2="{fmt(y1)}">'
        f'<stop offset="0" stop-color="#fff"/>'
        f'<stop offset="{fmt(f0 + 0.02)}" stop-color="#fff"/>'
        f'<stop offset="{fmt(f0 + 0.14)}" stop-color="#404040"/>'
        f'<stop offset="{fmt(f1 - 0.14)}" stop-color="#404040"/>'
        f'<stop offset="{fmt(f1 - 0.02)}" stop-color="#fff"/>'
        f'<stop offset="1" stop-color="#fff"/></linearGradient>'
        f'<mask id="relief-mask" maskUnits="userSpaceOnUse" x="-100" y="-100" width="1000" height="400">'
        f'<rect x="-100" y="-100" width="1000" height="400" fill="url(#relief-fade)"/></mask>'
    )
    body = (
        # The sleeve lies directly on the pair: no clear rim at its ends.
        pbody
        + f'<path d="{outline}" fill="{th.sleeve}"/>'
        f'<g clip-path="url(#sleeve-clip)"><g mask="url(#relief-mask)">{rbody}</g></g>'
        + stroke(top.path(rim_x, -4.8), "url(#rim-fade)", 3.2, "round", ' opacity="0.55"')
        + f'<path d="{t.d}" fill="{th.sleeve_text}"/>'
    )
    return svg_doc((1, 27, 626, 86), defs, body, "ShrinkWrap")


def main() -> None:
    for name, fn in (("wire_weaver_logo", wire_weaver), ("shrink_wrap_logo", shrink_wrap)):
        for suffix, th in (("", LIGHT), ("-dark", DARK)):
            svg = OUT / f"{name}{suffix}.svg"
            svg.write_text(fn(th))
            subprocess.run(
                ["rsvg-convert", "-z", "2", "-o", str(svg.with_suffix(".png")), str(svg)], check=True
            )
            print(f"wrote {svg.relative_to(OUT.parent.parent)} and .png")


if __name__ == "__main__":
    main()
