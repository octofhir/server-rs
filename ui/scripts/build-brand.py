"""Generate OctoFHIR brand assets (E3 mark, E1 small mark, lockups, favicons).

Requires fonttools + brotli and rsvg-convert + ImageMagick on PATH.
Usage: python build-brand.py packages/ui-kit/src/shared/assets/fonts/jetbrains-mono/jetbrains-mono-latin-wght-normal.woff2 ui/public/branding
"""
import subprocess
import sys
from pathlib import Path

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

FONT, OUT = sys.argv[1], Path(sys.argv[2])
OUT.mkdir(parents=True, exist_ok=True)

MARIGOLD = "#F2B630"
MARIGOLD_ON_LIGHT = "#B98309"
INK = "#16181D"
PAPER = "#ECEEF2"

RING = '<circle cx="11" cy="19" r="7.2" fill="none" stroke="{c}" stroke-width="4.4"/>'
SLASH = '<path d="M21.6 27.6 27.6 5.4" fill="none" stroke="{c}" stroke-width="4.4" stroke-linecap="round"/>'
PUPIL = '<rect x="8.6" y="17.8" width="4.8" height="2.4" rx="1.2" fill="{c}"/>'


def glyph(color, eye=True):
    parts = [RING.format(c=color), SLASH.format(c=color)]
    if eye:
        parts.append(PUPIL.format(c=color))
    return "".join(parts)


def svg(view_box, body, title="OctoFHIR"):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view_box}" role="img" aria-label="{title}">'
        f"<title>{title}</title>{body}</svg>\n"
    )


# Glyph bbox (stroke included) is x 1.6..29.8, y 3.2..29.8 → centre (15.7, 16.5).
def tile(eye, scale, radius):
    tx, ty = 16 - scale * 15.7, 16 - scale * 16.5
    return (
        f'<rect width="32" height="32" rx="{radius}" fill="{MARIGOLD}"/>'
        f'<g transform="translate({tx:.2f} {ty:.2f}) scale({scale})">{glyph(INK, eye)}</g>'
    )


def wordmark_paths():
    """Outline 'octo/fhir' in JetBrains Mono 600; returns (svg path for 'octo', '/', 'fhir', advance)."""
    font = TTFont(FONT)
    font = instantiateVariableFont(font, {"wght": 600})
    cmap, gs = font.getBestCmap(), font.getGlyphSet()
    upm = font["head"].unitsPerEm
    out, x = {"octo": "", "/": "", "fhir": ""}, 0
    bounds = BoundsPen(gs)
    for i, ch in enumerate("octo/fhir"):
        name = cmap[ord(ch)]
        key = "octo" if i < 4 else "/" if i == 4 else "fhir"
        pen = SVGPathPen(gs)
        # flip y: font units are y-up
        gs[name].draw(TransformPen(pen, (1, 0, 0, -1, x, 0)))
        out[key] += pen.getCommands()
        gs[name].draw(TransformPen(bounds, (1, 0, 0, 1, x, 0)))
        x += gs[name].width
    return out, x, bounds.bounds, upm


def lockup(text_color, slash_color, mark_color):
    paths, advance, (x0, y0, x1, y1), upm = wordmark_paths()
    fs = 17.5  # text size in mark units (mark = 32)
    s = fs / upm
    gap = 7
    text_mid = (y0 + y1) / 2 * s  # y-up
    ty = 16.5 + text_mid  # baseline so text centre sits on mark centre
    tx = 32 + gap
    width = tx + advance * s + 1
    body = (
        glyph(mark_color)
        + f'<g transform="translate({tx} {ty:.3f}) scale({s:.5f})">'
        f'<path fill="{text_color}" d="{paths["octo"]}"/>'
        f'<path fill="{slash_color}" d="{paths["/"]}"/>'
        f'<path fill="{text_color}" d="{paths["fhir"]}"/></g>'
    )
    return svg(f"0 0 {width:.2f} 32", body)


files = {
    "mark.svg": svg("0 0 32 32", glyph(MARIGOLD)),
    "mark-ink.svg": svg("0 0 32 32", glyph(INK)),
    "mark-white.svg": svg("0 0 32 32", glyph("#FFFFFF")),
    "mark-small.svg": svg("0 0 32 32", glyph(MARIGOLD, eye=False)),
    "favicon.svg": svg("0 0 32 32", tile(eye=False, scale=0.68, radius=7.5)),
    "app-icon.svg": svg("0 0 32 32", tile(eye=True, scale=0.62, radius=7.5)),
    "apple-touch-icon.svg": svg("0 0 32 32", tile(eye=True, scale=0.58, radius=0)),
    "safari-pinned-tab.svg": svg("0 0 32 32", glyph("#000000", eye=False)),
    "logo-light.svg": lockup(INK, MARIGOLD_ON_LIGHT, MARIGOLD),
    "logo-dark.svg": lockup(PAPER, MARIGOLD, MARIGOLD),
    "logo-mono-ink.svg": lockup(INK, INK, INK),
    "logo-mono-white.svg": lockup("#FFFFFF", "#FFFFFF", "#FFFFFF"),
}
for name, content in files.items():
    (OUT / name).write_text(content)

rasters = [
    ("favicon.svg", "favicon-16x16.png", 16),
    ("favicon.svg", "favicon-32x32.png", 32),
    ("favicon.svg", "favicon-48x48.png", 48),
    ("apple-touch-icon.svg", "apple-touch-icon.png", 180),
    ("app-icon.svg", "icon-192.png", 192),
    ("app-icon.svg", "icon-512.png", 512),
]
for src, dst, size in rasters:
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), str(OUT / src), "-o", str(OUT / dst)], check=True)
subprocess.run(
    ["magick", *(str(OUT / f"favicon-{s}x{s}.png") for s in (16, 32, 48)), str(OUT / "favicon.ico")], check=True
)
(OUT / "favicon-48x48.png").unlink()
print("\n".join(sorted(p.name for p in OUT.iterdir())))
