"""Builds GuidoNoSpace.ttf: the digits, each 1500 units wide on a 1000 em,
and no space — the shape of an icon font such as Symbols Nerd Font. Made here,
from nothing, so a test can ask what a family without a space is measured in.

    python3 tests/assets/make_no_space_fixture.py
"""

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def box(width):
    pen = TTGlyphPen(None)
    pen.moveTo((50, 0))
    pen.lineTo((50, 700))
    pen.lineTo((50 + width, 700))
    pen.lineTo((50 + width, 0))
    pen.closePath()
    return pen.glyph()


digits = [f"d{n}" for n in range(10)]
order = [".notdef"] + digits
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x30 + n: f"d{n}" for n in range(10)})
fb.setupGlyf({name: box(1400) for name in order})
fb.setupHorizontalMetrics({name: (1500, 50) for name in order})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Guido No Space", "styleName": "Regular", "psName": "GuidoNoSpace-Regular"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200, usWeightClass=400)
fb.setupPost()
fb.save("tests/assets/GuidoNoSpace.ttf")
