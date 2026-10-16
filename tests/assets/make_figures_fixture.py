"""Builds GuidoFigures-Regular.ttf and GuidoFigures-Italic.ttf: one family,
"Guido Figures", whose upright digits are proportional — digit n is a box
400 + 40n units wide on a 1000 em, so a "1" is wider than a "0" — and whose
`tnum` feature swaps every one of them for a box 600 units wide. Its italic
face is a face of its own, with other boxes: slanted, and half as tall, so a
picture says which face drew it and a shear of the upright one cannot pass
for it. The italic face's ascent is 1000 rather than 800, so its line is
taller and a test can say which face the line height came from.

Made here, from nothing, so a test can ask what a feature and a style do
without vendoring anybody's font.

    python3 tests/assets/make_figures_fixture.py
"""

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def box(width, height, slant):
    pen = TTGlyphPen(None)
    pen.moveTo((50, 0))
    pen.lineTo((50 + slant, height))
    pen.lineTo((50 + slant + width, height))
    pen.lineTo((50 + width, 0))
    pen.closePath()
    return pen.glyph()


digits = [f"d{n}" for n in range(10)]
tabular = [f"d{n}.tnum" for n in range(10)]
order = [".notdef", "space"] + digits + tabular


def build(style, height, slant, ascent, path):
    advance = {".notdef": 500, "space": 300}
    advance.update({f"d{n}": 400 + 40 * n for n in range(10)})
    advance.update({f"d{n}.tnum": 600 for n in range(10)})

    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x20: "space", **{0x30 + n: f"d{n}" for n in range(10)}})
    fb.setupGlyf(
        {
            name: box(advance[name] - 100, height, slant) if name != "space" else TTGlyphPen(None).glyph()
            for name in order
        }
    )
    fb.setupHorizontalMetrics({name: (advance[name], 50) for name in order})
    fb.setupHorizontalHeader(ascent=ascent, descent=-200)
    italic = style == "Italic"
    fb.setupNameTable(
        {
            "familyName": "Guido Figures",
            "styleName": style,
            "psName": f"GuidoFigures-{style}",
        }
    )
    fb.setupOS2(
        version=4,
        sTypoAscender=ascent,
        sTypoDescender=-200,
        sTypoLineGap=0,
        usWinAscent=ascent,
        usWinDescent=200,
        usWeightClass=400,
        # Bit 0 is italic, bit 6 regular; bit 7 says the typo metrics are the ones to use.
        fsSelection=(1 << 0 if italic else 1 << 6) | 1 << 7,
    )
    fb.setupPost(italicAngle=-12 if italic else 0)
    fb.font["head"].macStyle = 0b10 if italic else 0
    fb.addOpenTypeFeatures(
        "feature tnum { sub [%s] by [%s]; } tnum;" % (" ".join(digits), " ".join(tabular))
    )
    fb.save(path)


build("Regular", 700, 0, 800, "tests/assets/GuidoFigures-Regular.ttf")
build("Italic", 350, 120, 1000, "tests/assets/GuidoFigures-Italic.ttf")
