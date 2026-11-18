"""Zeichnet das App-Symbol von Tephra und schreibt die Dateien für Android und Windows.

Geometrie auf einem Raster von 108 Einheiten (wie das adaptive Symbol von Android):
Eingabezeichen ">" und Schreibmarke "_" in Türkis, darüber aufsteigende Bruchstücke in Bernstein.
Dieselbe Geometrie steht in `android/app/src/main/res/drawable/ic_launcher_foreground.xml`
und in `lib/src/ui/logo.dart`; wer sie hier ändert, ändert sie auch dort.

Braucht Pillow. Aufruf im Ordner `app`:  python tool/make_icons.py . vorschau.png"""
import sys
from PIL import Image, ImageDraw

APP, PREVIEW = sys.argv[1], sys.argv[2]

BACKGROUND = (14, 21, 19, 255)      # #0E1513
PROMPT = (122, 219, 200, 255)       # #7ADBC8
SHARD = (255, 180, 84, 255)         # #FFB454
STROKE = 7.5
CHEVRON = [(33, 41), (50, 55), (33, 69)]
CURSOR = [(57, 69), (77, 69)]
SHARDS = [
    [(61, 40), (67, 46), (61, 52), (55, 46)],
    [(69, 30), (79, 34), (72, 41)],
    [(63, 23), (66.5, 26.5), (63, 30), (59.5, 26.5)],
    [(81, 22), (84, 25), (81, 28), (78, 25)],
]
# Ausschnitt des Rasters, der bei Symbolen mit eigener Kachel die ganze Fläche füllt.
TILE_FROM, TILE_TO = 14, 94


def glyph(draw, scale, dx, dy, prompt=PROMPT, shard=SHARD):
    def p(point):
        return ((point[0] - dx) * scale, (point[1] - dy) * scale)

    width = STROKE * scale
    for line in (CHEVRON, CURSOR):
        points = [p(point) for point in line]
        draw.line(points, fill=prompt, width=round(width), joint='curve')
        for x, y in points:
            draw.ellipse([x - width / 2, y - width / 2, x + width / 2, y + width / 2], fill=prompt)
    for polygon in SHARDS:
        draw.polygon([p(point) for point in polygon], fill=shard)


def tile(size):
    """Symbol mit eigener, abgerundeter Kachel (Windows, ältere Android-Fassungen)."""
    big = size * 8 if size < 256 else size * 4
    image = Image.new('RGBA', (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    # Ein feiner heller Rand hält die dunkle Kachel auch auf dunklem Grund sichtbar.
    draw.rounded_rectangle([0, 0, big - 1, big - 1], radius=big * 0.22, fill=BACKGROUND,
                           outline=(255, 255, 255, 40), width=max(1, round(big * 0.012)))
    glyph(draw, big / (TILE_TO - TILE_FROM), TILE_FROM, TILE_FROM)
    return image.resize((size, size), Image.LANCZOS)


for name, size in {'mdpi': 48, 'hdpi': 72, 'xhdpi': 96, 'xxhdpi': 144, 'xxxhdpi': 192}.items():
    tile(size).save(f'{APP}/android/app/src/main/res/mipmap-{name}/ic_launcher.png', optimize=True)

sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
tile(256).save(f'{APP}/windows/runner/resources/app_icon.ico', sizes=[(s, s) for s in sizes],
               append_images=[tile(s) for s in sizes[:-1]])

# Vorschau: groß, klein und das adaptive Symbol in Kreisform, auf hellem und dunklem Grund.
preview = Image.new('RGBA', (1100, 420), (236, 240, 238, 255))
ImageDraw.Draw(preview).rectangle([550, 0, 1100, 420], fill=(24, 26, 28, 255))
for offset in (0, 550):
    preview.alpha_composite(tile(256), (offset + 24, 24))
    preview.alpha_composite(tile(96), (offset + 304, 24))
    preview.alpha_composite(tile(48), (offset + 304, 144))
    preview.alpha_composite(tile(32), (offset + 372, 144))
    preview.alpha_composite(tile(16), (offset + 424, 144))
    big = 4 * 108
    adaptive = Image.new('RGBA', (big, big), BACKGROUND)
    glyph(ImageDraw.Draw(adaptive), 4, 0, 0)
    mask = Image.new('L', (big, big), 0)
    ImageDraw.Draw(mask).ellipse([18 * 4, 18 * 4, 90 * 4, 90 * 4], fill=255)
    adaptive.putalpha(mask)
    adaptive = adaptive.crop((18 * 4, 18 * 4, 90 * 4, 90 * 4)).resize((120, 120), Image.LANCZOS)
    preview.alpha_composite(adaptive, (offset + 304, 220))
preview.save(PREVIEW)
print('ok')
