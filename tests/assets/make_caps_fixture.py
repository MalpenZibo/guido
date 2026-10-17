"""Builds GuidoCaps.ttf: a capital H that is a box exactly as tall as the cap
height the face declares (700 units on a 1000 em), an x exactly as tall as its
x-height (500), a p that hangs 150 below the baseline, and a space. Its ascent
is 1100 and its descent 150, so the room above its capitals is far larger than
the room below them — the opposite of DejaVu Sans Mono, which declares no cap
height at all — and a text centred by its line box sits visibly low.

Made here, from nothing, so a test can find a capital's ink to the pixel and
know where the face says it ends.

    python3 tests/assets/make_caps_fixture.py
"""

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def box(width, bottom, top):
    pen = TTGlyphPen(None)
    pen.moveTo((50, bottom))
    pen.lineTo((50, top))
    pen.lineTo((50 + width, top))
    pen.lineTo((50 + width, bottom))
    pen.closePath()
    return pen.glyph()


order = [".notdef", "space", "H", "x", "p"]
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x20: "space", ord("H"): "H", ord("x"): "x", ord("p"): "p"})
fb.setupGlyf(
    {
        ".notdef": box(400, 0, 700),
        "space": TTGlyphPen(None).glyph(),
        "H": box(500, 0, 700),
        "x": box(400, 0, 500),
        "p": box(400, -150, 500),
    }
)
fb.setupHorizontalMetrics(
    {".notdef": (500, 50), "space": (300, 0), "H": (600, 50), "x": (500, 50), "p": (500, 50)}
)
fb.setupHorizontalHeader(ascent=1100, descent=-150)
fb.setupNameTable({"familyName": "Guido Caps", "styleName": "Regular", "psName": "GuidoCaps-Regular"})
fb.setupOS2(
    version=4,
    sTypoAscender=1100,
    sTypoDescender=-150,
    sTypoLineGap=0,
    usWinAscent=1100,
    usWinDescent=150,
    usWeightClass=400,
    sCapHeight=700,
    sxHeight=500,
    # Bit 6 is regular; bit 7 says the typo metrics are the ones to use.
    fsSelection=1 << 6 | 1 << 7,
)
fb.setupPost()
fb.save("tests/assets/GuidoCaps.ttf")
