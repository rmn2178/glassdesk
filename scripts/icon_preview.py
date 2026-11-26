"""Builds assets/brand/muelle-preview.png from assets/brand/muelle-1024.png.

Shows the icon on dark and light backgrounds at the sizes Windows uses. It
never modifies the icon itself.

Usage: python scripts/icon_preview.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

BRAND = Path(__file__).resolve().parent.parent / "assets" / "brand"
SIZES = (256, 96, 48, 32, 16)


def main() -> None:
    icon = Image.open(BRAND / "muelle-1024.png").convert("RGBA")
    sheet = Image.new("RGB", (760, 620), (32, 32, 36))
    ImageDraw.Draw(sheet).rectangle((0, 310, 760, 620), fill=(243, 243, 246))
    for row in (0, 1):
        x = 30
        for size in SIZES:
            small = icon.resize((size, size), Image.LANCZOS)
            sheet.paste(small, (x, row * 310 + (310 - size) // 2), small)
            x += size + 40
    sheet.save(BRAND / "muelle-preview.png")


if __name__ == "__main__":
    main()
