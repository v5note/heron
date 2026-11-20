#!/usr/bin/env python3
"""Render the rail logos (light + dark theme) from their masters.

Both masters are 1024-class artwork that is not shaped like the 34 px tile the rail needs:

  light  docs/icons/icon-master-2.png  a white heron on a black plate. Drawn as a transparent
         PNG it is white ink on transparency, which disappears on the light rail. It is rendered
         on a neutral dark plate instead, which keeps the white bird intact and matches the dark
         logo's plate, so the two themes read as one logo in two palettes.
  dark   docs/icons/icon-master-6.png  near-black ink on a fully transparent plate. On the dark
         rail (#0b0a09) that is invisible; the ink is recoloured to warm parchment and the plate
         filled with the same neutral dark.

Both are then scaled to the tile and recentred on the bird's bounding box, because the raw
canvases carry asymmetric padding (the light one runs off the right edge) and would otherwise
sit visibly off-centre in the square.

Usage: python3 scripts/make-rail-logo.py
"""
from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "ui/src/assets"

PLATE = (58, 54, 48)           # neutral dark, one step above the rail --panel #0b0a09
PARCHMENT = (213, 204, 188)    # dark theme: the light bird's counterpart
TILE = 272                     # 2x the 34 px CSS tile, so retina never upscales
FILL = 0.86                    # bird height as a share of the tile


def split(arr: np.ndarray) -> dict[str, np.ndarray]:
    r, g, b, a = (arr[..., i].astype(int) for i in range(4))
    peak = np.maximum(np.maximum(r, g), b)
    spread = peak - np.minimum(np.minimum(r, g), b)
    solid = a > 8
    warm = solid & (spread >= 60)          # orange beak and eye, always kept as drawn
    light_ink = solid & ~warm & (peak >= 200)
    dark_ink = solid & ~warm & (peak <= 60)
    return {"alpha": a, "warm": warm, "light": light_ink, "dark": dark_ink}


def render(source: Path, ink_rgb: tuple[int, int, int], ink_mask: str, target: Path) -> None:
    src = Image.open(source).convert("RGBA")
    arr = np.array(src)
    parts = split(arr)

    body = np.zeros_like(arr)
    body[parts["alpha"] <= 8] = (0, 0, 0, 0)
    # Warm colours and every semi-transparent edge keep their own pixels: that is what preserves
    # the beak, the eye, and the anti-aliased rim of the silhouette at small sizes.
    keep = (parts["alpha"] > 8) & ~parts["dark" if ink_mask == "dark" else "light"]
    body[keep] = arr[keep]
    body[parts[ink_mask]] = (*ink_rgb, 255)
    body[parts["alpha"] <= 8] = (0, 0, 0, 0)

    bird = body[..., 3] > 8
    ys, xs = np.where(bird)
    x0, x1, y0, y1 = xs.min(), xs.max(), ys.min(), ys.max()
    h = y1 - y0 + 1
    scale = TILE * FILL / h

    art = Image.fromarray(body).resize(
        (round(src.width * scale), round(src.height * scale)), Image.LANCZOS
    )
    cx = (x0 + x1) / 2 * scale
    cy = (y0 + y1) / 2 * scale
    canvas = Image.new("RGBA", (TILE, TILE), (*PLATE, 255))
    canvas.alpha_composite(art, (round(TILE / 2 - cx), round(TILE / 2 - cy)))

    # Rounded tile, so the CSS border-radius has something opaque to clip.
    from PIL import ImageDraw

    mask = Image.new("L", (TILE, TILE), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, TILE - 1, TILE - 1], radius=round(TILE * 0.27), fill=255
    )
    out = Image.new("RGBA", (TILE, TILE), (0, 0, 0, 0))
    out.paste(canvas, (0, 0), mask)
    out.save(target, optimize=True)
    print(f"wrote {target.relative_to(ROOT)}")


def main() -> None:
    render(ROOT / "docs/icons/icon-master-2.png", (255, 255, 255), "light", OUT / "logo-light.png")
    render(ROOT / "docs/icons/icon-master-6.png", PARCHMENT, "dark", OUT / "logo-dark.png")


if __name__ == "__main__":
    main()
