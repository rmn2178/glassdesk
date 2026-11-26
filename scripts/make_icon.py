"""Draws the Muelle icon: a glass lotus on a purple-to-orange squircle.

Everything is vector-defined, edges get analytic anti-aliasing, and the
result is rendered at 2x and downsampled, so edges are crisp and the area outside the squircle is fully transparent (no halo).

Writes assets/brand/muelle-1024.png and assets/brand/muelle.ico, and copies the
.ico to the Settings app. Run scripts/icon_preview.py afterwards.

Usage: python scripts/make_icon.py
"""

import shutil
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "assets" / "brand"
SETTINGS_ICON = ROOT / "apps" / "settings" / "src-tauri" / "icons" / "icon.ico"

SS = 2  # supersampling factor; edges also get analytic coverage
N = 1024 * SS
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)

# Squircle: same proportions as a macOS icon tile, slightly larger so it
# reads well next to Windows icons.
MARGIN = 72 / 1024
EXPONENT = 5.0

# Colour ramp along the gradient axis (0 = top-left, 1 = bottom-right).
RAMP = (
    (0.00, (190, 150, 252)),
    (0.30, (124, 78, 226)),
    (0.52, (110, 60, 196)),
    (0.76, (238, 98, 52)),
    (1.00, (255, 150, 40)),
)


def grid() -> tuple[np.ndarray, np.ndarray]:
    """Pixel centres in unit coordinates (0..1)."""
    axis = (np.arange(N, dtype=np.float32) + 0.5) / N
    return np.meshgrid(axis, axis)


def coverage(distance: np.ndarray) -> np.ndarray:
    """Signed distance inside a shape (unit coordinates) to pixel coverage."""
    return np.clip(distance * N + 0.5, 0.0, 1.0).astype(np.float32)


def squircle(x: np.ndarray, y: np.ndarray, inset: float = 0.0) -> np.ndarray:
    half = 0.5 - MARGIN - inset
    u = np.abs(x - 0.5) / half
    v = np.abs(y - 0.5) / half
    radius = (u**EXPONENT + v**EXPONENT) ** (1 / EXPONENT)
    return coverage((1 - radius) * half)


def ramp(t: np.ndarray) -> np.ndarray:
    t = np.clip(t, 0.0, 1.0)
    out = np.zeros(t.shape + (3,), np.float32)
    for (t0, c0), (t1, c1) in zip(RAMP, RAMP[1:]):
        inside = (t >= t0) & (t <= t1)
        k = ((t - t0) / (t1 - t0))[inside][:, None]
        out[inside] = np.array(c0) * (1 - k) + np.array(c1) * k
    return out / 255.0


def over(dst: np.ndarray, colour, alpha: np.ndarray) -> None:
    """Composites a colour (rgb tuple or HxWx3 array) with per-pixel alpha."""
    a = alpha[..., None]
    dst[:] = dst * (1 - a) + np.asarray(colour, np.float32) * a


def lens(x, y, base, tip, half_width):
    """A petal: the intersection of two circles, pointed at base and tip.

    Returns (inside mask, along-axis position 0..1, distance to the edge).
    """
    bx, by = base
    tx, ty = tip
    length = np.hypot(tx - bx, ty - by)
    ux, uy = (tx - bx) / length, (ty - by) / length
    along = (x - bx) * ux + (y - by) * uy
    across = np.abs(-(x - bx) * uy + (y - by) * ux)
    radius = ((length / 2) ** 2 + half_width**2) / (2 * half_width)
    centre_offset = radius - half_width
    # Distance from the arc on this side: the arc's centre lies across the axis.
    dist_to_centre = np.hypot(along - length / 2, across + centre_offset)
    edge = radius - dist_to_centre
    return coverage(edge), along / length, edge


def draw(stroke: float, petal_boost: float) -> Image.Image:
    """Renders the master image. Small icon sizes use a bolder variant: thicker
    outlines and more opaque petals, so the lotus still reads at 16-48 px."""
    x, y = grid()
    img = np.zeros((N, N, 3), np.float32)
    tile = squircle(x, y)

    # Background gradient, tilted so purple holds the left and orange the right.
    t = (x * 0.82 + y * 0.42) / 1.24
    img[:] = ramp(t)
    # Soft light from the top-left, and a gentle darkening at the bottom.
    glow = np.clip(1 - np.hypot(x - 0.18, y - 0.12) / 0.75, 0, 1) ** 2
    over(img, (1, 1, 1), glow * 0.22)
    over(img, (0.25, 0.1, 0.35), np.clip((y - 0.55) / 0.45, 0, 1) * 0.12)

    # A faint, soft wave across the lower half.
    wave = 0.70 - 0.16 * np.sin((x - 0.05) * 3.1)
    over(img, (1, 0.85, 0.7), np.clip((y - wave) / 0.06, 0, 1) * 0.08)

    # Petals: back (centre) first, then the two sides over it.
    base = (0.5, 0.75)
    petals = (
        (base, (0.5, 0.225), 0.14, 0.24),
        (base, (0.195, 0.4), 0.098, 0.28),
        (base, (0.805, 0.4), 0.098, 0.28),
    )
    for petal_base, tip, half_width, opacity in petals:
        inside, along, edge = lens(x, y, petal_base, tip, half_width)
        # Frosted glass: whiter towards the tip, clearer at the base.
        fill = inside * (opacity + petal_boost) * (0.55 + 0.45 * np.clip(along, 0, 1))
        over(img, (1, 1, 1), fill.astype(np.float32))
        rim = coverage(stroke / 2 - np.abs(edge)) * (0.55 + 0.35 * np.clip(along, 0, 1))
        over(img, (1, 1, 1), rim.astype(np.float32))

    # Glass rim just inside the tile edge, brighter on top.
    ring = tile - squircle(x, y, inset=3 / 1024)
    over(img, (1, 1, 1), ring * (0.25 + 0.35 * (1 - y)))
    # Specular sheen over the top half.
    over(img, (1, 1, 1), np.clip(0.5 - y, 0, 0.5) * 0.18)

    rgba = np.dstack([np.clip(img, 0, 1), tile])
    # Premultiply before downsampling so transparent pixels add no colour fringe.
    rgba[..., :3] *= rgba[..., 3:4]
    return Image.fromarray((rgba * 255 + 0.5).astype(np.uint8))


def shrink(master: Image.Image, size: int) -> Image.Image:
    small = np.asarray(master.resize((size, size), Image.LANCZOS)).astype(np.float32) / 255
    alpha = np.clip(small[..., 3:4], 0, 1)
    colour = np.where(alpha > 0, small[..., :3] / np.maximum(alpha, 1e-6), 0)
    out = np.dstack([np.clip(colour, 0, 1), alpha])
    return Image.fromarray((out * 255 + 0.5).astype(np.uint8))


def variant(size: int, fine: Image.Image, medium: Image.Image, bold: Image.Image) -> Image.Image:
    """The master that keeps the lotus outline at least about one pixel wide."""
    if size <= 48:
        return bold
    # The dock shows the 256 px image at 60-120 px: with the fine outline
    # (0.3 px there) the lotus looked blurred and washed out.
    return medium if size <= 256 else fine


def main() -> None:
    fine = draw(stroke=3.2 / 1024, petal_boost=0.0)
    medium = draw(stroke=13 / 1024, petal_boost=0.07)
    bold = draw(stroke=22 / 1024, petal_boost=0.14)

    shrink(fine, 1024).save(BRAND / "muelle-1024.png", optimize=True)
    frames = [shrink(variant(size, fine, medium, bold), size) for size in ICO_SIZES]
    frames[-1].save(
        BRAND / "muelle.ico",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=frames[:-1],
        bitmap_format="png",
    )
    shutil.copyfile(BRAND / "muelle.ico", SETTINGS_ICON)
    print("wrote muelle-1024.png, muelle.ico and the Settings icon")


if __name__ == "__main__":
    main()
