"""The `bits` fence: a bit layout drawn as coloured bits in byte cells, with a table of its fields.

One field per line: its kind, its bits (groups of `0` and `1`, spaces between groups are kept in the table), then
its label, which starts at the first word that isn't only `0` and `1`. A `bytes` line gives the bytes the fields
make, MSB first, as `shrink_wrap` writes them; the build fails when they don't match.

    ```bits
    head  0100      n = 4
    run   0001 0    v.prefix run: Milli, end
    value 0000110011100100  3300
    pad   00        padding
    bytes 42 11 ..
    ```

Kinds: `head` (counts, field counts, codec headers), `run` (tag runs), `value` (full values), `delta` (residuals
and other coded data), `width` (block widths), `pad` (padding). Wired up in zensical.toml as a custom fence of
pymdownx.superfences; the colours are in docs/stylesheets/vhrd.css (`.ww-bits`).
"""

import html
import re

from pymdownx.superfences import SuperFencesException

KINDS = {
    "head": "header",
    "run": "run",
    "value": "value",
    "delta": "residuals",
    "width": "block width",
    "pad": "padding",
}

_BITS = re.compile(r"^[01]+$")
_CODE = re.compile(r"`([^`]+)`")


class Field:
    def __init__(self, kind, groups, label, line):
        self.kind = kind
        self.groups = groups
        self.label = label
        self.line = line

    @property
    def bits(self):
        return "".join(self.groups)


def parse(source):
    """Fields and the expected bytes (or None) of a fence's source."""
    fields = []
    expected = None
    for n, raw in enumerate(source.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        words = line.split()
        kind, rest = words[0], words[1:]
        if kind == "bytes":
            try:
                expected = bytes(int(w, 16) for w in rest)
            except ValueError as e:
                raise SuperFencesException(f"bits fence, line {n}: bad byte in {line!r}: {e}") from e
            continue
        if kind not in KINDS:
            raise SuperFencesException(
                f"bits fence, line {n}: unknown kind {kind!r}, one of {', '.join(KINDS)} or bytes"
            )
        groups = []
        while rest and _BITS.match(rest[0]):
            groups.append(rest.pop(0))
        if not groups:
            raise SuperFencesException(f"bits fence, line {n}: no bits in {line!r}")
        fields.append(Field(kind, groups, " ".join(rest), n))
    if not fields:
        raise SuperFencesException("bits fence: no fields")
    return fields, expected


def pack(bits):
    """Bits, MSB first, as bytes; the last byte padded with zeros."""
    padded = bits + "0" * (-len(bits) % 8)
    return bytes(int(padded[i : i + 8], 2) for i in range(0, len(padded), 8))


def _label(text):
    """A label as HTML: escaped, `code` spans kept."""
    return _CODE.sub(lambda m: f"<code>{m.group(1)}</code>", html.escape(text, quote=False))


def render(source):
    fields, expected = parse(source)
    bits = "".join(f.bits for f in fields)
    got = pack(bits)
    if expected is not None:
        if len(bits) % 8:
            raise SuperFencesException(
                f"bits fence: {len(bits)} bits are not whole bytes, add the padding as a pad field"
            )
        if got != expected:
            raise SuperFencesException(
                f"bits fence: the fields make {got.hex(' ')}, the bytes line says {expected.hex(' ')}"
            )

    # each bit with its field
    owner = []
    offsets = []
    at = 0
    for i, f in enumerate(fields):
        offsets.append(at)
        owner.extend([i] * len(f.bits))
        at += len(f.bits)

    out = ['<div class="ww-bits">']
    summary = f"{len(bits)} bits in {len(got)} bytes: {got.hex(' ')}"
    out.append(f'<div class="ww-bits__strip" role="img" aria-label="{html.escape(summary)}">')
    for b in range(len(got)):
        out.append('<span class="ww-bits__byte">')
        out.append('<span class="ww-bits__cells">')
        for k in range(b * 8, b * 8 + 8):
            if k < len(bits):
                f = fields[owner[k]]
                start = " ww-bit--start" if k == offsets[owner[k]] else ""
                title = html.escape(f"bit {k}: {f.label.replace('`', '') or KINDS[f.kind]}")
                out.append(f'<span class="ww-bit ww-bit--{f.kind}{start}" title="{title}">{bits[k]}</span>')
            else:
                out.append('<span class="ww-bit ww-bit--none" title="not written">0</span>')
        out.append("</span>")
        out.append(f'<span class="ww-bits__hex">{got[b]:02x}</span>')
        out.append("</span>")
    out.append("</div>")

    used = [k for k in KINDS if any(f.kind == k for f in fields)]
    out.append('<div class="ww-bits__legend">')
    for k in used:
        out.append(f'<span class="ww-bits__key"><span class="ww-bit ww-bit--{k}"></span>{KINDS[k]}</span>')
    out.append("</div>")

    out.append('<table class="ww-bits__fields"><thead><tr><th>bit</th><th>bits</th><th>field</th></tr></thead><tbody>')
    for f, at in zip(fields, offsets):
        chips = " ".join(html.escape(g) for g in f.groups)
        out.append(
            f'<tr><td class="ww-bits__at">{at}</td>'
            f'<td><span class="ww-bits__chip ww-bit--{f.kind}">{chips}</span></td>'
            f"<td>{_label(f.label)}</td></tr>"
        )
    out.append("</tbody></table>")
    out.append("</div>")
    return "".join(out)


def fence(source, language, class_name, options, md, **kwargs):
    """The pymdownx.superfences format function of the `bits` fence."""
    return render(source)
