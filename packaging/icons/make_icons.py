"""File-type icons for Explorer: a page with a folded corner, the brand's
dotted line, and the format's name on a band. Writes assets/icons/*.ico.

    python3 packaging/icons/make_icons.py [--preview out.png]

Drawn at 4x and scaled down per size; at 16 and 24 px the name is left out
(unreadable) and only the band's colour tells formats apart.
"""
import sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONT = ROOT / "assets/fonts/InterTight-ExtraBold.ttf"
OUT = ROOT / "assets/icons"

INK = (17, 17, 17, 255)
PAPER = (244, 242, 236, 255)

# (file stem, label, band colour). Muted colours so formats can be told
# apart in a folder at a glance, kept close to the black-and-white brand.
FORMATS = [
    ("jpg", "JPG", (17, 17, 17)),
    ("png", "PNG", (38, 84, 124)),
    ("gif", "GIF", (124, 58, 110)),
    ("webp", "WEBP", (46, 110, 78)),
    ("avif", "AVIF", (176, 82, 44)),
    ("jxl", "JXL", (90, 70, 150)),
    ("heic", "HEIC", (60, 60, 60)),
    ("tif", "TIFF", (110, 96, 60)),
    ("raw", "RAW", (150, 40, 40)),
    ("psd", "PSD", (24, 90, 140)),
    ("svg", "SVG", (180, 120, 20)),
    ("cbz", "CBZ", (70, 70, 70)),
    ("image", "IMG", (110, 110, 110)),
]

SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]


def draw(label, band, size):
    s = 4 * size
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    u = s / 32  # one unit = 1/32 of the icon
    # Page: portrait, centred, folded top-right corner.
    left, right, top, bottom = 5 * u, 27 * u, 1.5 * u, 30.5 * u
    fold = 7 * u
    line = max(1.0 * u, 4)
    page = [(left, top), (right - fold, top), (right, top + fold), (right, bottom), (left, bottom)]
    d.polygon(page, fill=PAPER, outline=INK)
    d.line(page + [page[0]], fill=INK, width=round(line), joint="curve")
    d.polygon([(right - fold, top), (right - fold, top + fold), (right, top + fold)], fill=INK)
    small = size <= 24
    # Band with the name.
    band_top = 17 * u if not small else 16 * u
    d.rectangle([left, band_top, right, bottom], fill=band + (255,))
    d.line(page + [page[0]], fill=INK, width=round(line), joint="curve")
    if not small:
        box_w = (right - left) - 3 * u
        box_h = (bottom - band_top) - 4.5 * u
        fs = int(box_h)
        font = ImageFont.truetype(str(FONT), fs)
        while font.getlength(label) > box_w and fs > 4:
            fs -= 2
            font = ImageFont.truetype(str(FONT), fs)
        cx, cy = (left + right) / 2, (band_top + bottom) / 2
        d.text((cx, cy), label, font=font, fill=PAPER, anchor="mm")
        # The brand: its "m" and dotted line above the band.
        if size >= 32:
            m = ImageFont.truetype(str(FONT), int(9 * u))
            d.text((left + 3 * u, band_top - 4.6 * u), "m", font=m, fill=INK, anchor="ls")
            y = band_top - 3.2 * u
            r = 0.55 * u
            n = 9
            for i in range(n):
                x = left + 3 * u + i * ((right - left - 6 * u) / (n - 1))
                d.ellipse([x - r, y - r, x + r, y + r], fill=INK)
    return img.resize((size, size), Image.LANCZOS)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for stem, label, band in FORMATS:
        frames = [draw(label, band, n) for n in SIZES]
        frames[-1].save(OUT / f"{stem}.ico", sizes=[(n, n) for n in SIZES],
                        append_images=frames[:-1])
    if "--preview" in sys.argv:
        out = Path(sys.argv[sys.argv.index("--preview") + 1])
        cols = len(FORMATS)
        rows = [256, 48, 32, 16]
        sheet = Image.new("RGBA", (cols * 140 + 20, 140 + 70 * 3 + 20), (255, 255, 255, 255))
        for i, (stem, label, band) in enumerate(FORMATS):
            x = 20 + i * 140
            sheet.alpha_composite(draw(label, band, 128), (x, 10))
            y = 150
            for n in rows[1:]:
                sheet.alpha_composite(draw(label, band, n), (x, y))
                y += 70
        sheet.save(out)


if __name__ == "__main__":
    main()
