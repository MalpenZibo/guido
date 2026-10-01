"""Builds GuidoVariable.ttf: a variable font with one `wght` axis (100-900,
default 400) and boxes for a space and the digits. Made here, from nothing,
so a test can ask what weight a variable face is shaped at without vendoring
anybody's font.

    python3 tests/assets/make_variable_fixture.py
"""

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables._g_l_y_f import Glyph
from fontTools.ttLib.tables import _g_v_a_r
from fontTools.ttLib.tables.TupleVariation import TupleVariation


def box(width):
    pen = TTGlyphPen(None)
    pen.moveTo((50, 0))
    pen.lineTo((50, 700))
    pen.lineTo((50 + width, 700))
    pen.lineTo((50 + width, 0))
    pen.closePath()
    return pen.glyph()


digits = [f"d{n}" for n in range(10)]
order = [".notdef", "space"] + digits
fb = FontBuilder(1000, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x20: "space", **{0x30 + n: f"d{n}" for n in range(10)}})
glyphs = {".notdef": box(400), "space": Glyph()}
glyphs.update({name: box(400) for name in digits})
fb.setupGlyf(glyphs)
fb.setupHorizontalMetrics({name: (600, 50) for name in order} | {"space": (600, 0)})
fb.setupHorizontalHeader(ascent=800, descent=-200)
fb.setupNameTable({"familyName": "Guido Variable", "styleName": "Regular", "psName": "GuidoVariable-Regular"})
fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200, usWeightClass=400)
fb.setupPost()
fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])
# The heavier the weight, the wider the box: so the axis does something.
variations = {}
for name in digits:
    coords = [(0, 0), (0, 0), (300, 0), (300, 0), (0, 0), (0, 0), (0, 0), (0, 0)]
    variations[name] = [TupleVariation({"wght": (0.0, 1.0, 1.0)}, coords)]
fb.setupGvar(variations)
fb.save("tests/assets/GuidoVariable.ttf")
