#!/usr/bin/env python3
"""Build the Heron app icons from the attached heron artwork.

The outer body follows the v5notes client 2 icon. The artwork scale and corner
rounding are tuned to the neighboring icons in the supplied screenshot:

  macOS  body = 824 / 1024 of the canvas (100 px inset on each side of a 1024
         master), continuous-superellipse corners with a 0.400 corner factor
         and a transparent margin around it.
  Windows body = the full canvas, so Windows Explorer, the taskbar, the Start
         menu and the MSIX tile assets show the artwork edge to edge like the
         reference Windows icon.
  Artwork = the supplied black silhouette with its orange beak and eye;
         white knock-outs stay white. It is 64% of the body width and up to
         84% of its height, with horizontal mass-centering and a slight lift.

Every file name matches the reference layout, so tauri.conf.json keeps pointing
at the same paths.

Usage: python3 scripts/make-icons.py [--source docs/icons/white-bg-black-silhouette/icon-master.png]
"""
from __future__ import annotations

import argparse
import io
import struct
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parent.parent

# --- reference geometry (v5notes client 2) -----------------------------------
SIZE = 1024                     # masters are 1024 x 1024
MAC_BODY = 824                  # macOS squircle: 103 px inset on every side
MAC_SQUIRCLE = 0.400            # larger corner radius to match neighboring icons
WIN_BODY = 1024                 # Windows: no margin, the OS keeps 4 px of every tile
WIN_SQUIRCLE = 0.400
ART_MAX_W = 0.64                # artwork box / body
ART_MAX_H = 0.84                # keep the heron visibly inset within the tile
OPTICAL_RISE = 0.006            # body-height fraction the artwork is lifted
DESPECKLE = 9                   # window (px) that tells a stray hair from detail
DESPECKLE_STEP = 60             # tone jump (0-255) that marks a stray hair
INK_GREY = 150                  # grey between black and white ink counts as ink

ICNS_TYPES = {
    16: b"icp4", 32: b"icp5", 64: b"icp6",
    128: b"ic07", 256: b"ic08", 512: b"ic09", 1024: b"ic10",
}
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)


# --- artwork ----------------------------------------------------------------
def _shift(mask: np.ndarray, dy: int, dx: int) -> np.ndarray:
    """Move a mask by (dy, dx), filling the gap with False."""
    out = np.zeros_like(mask)
    ys = slice(max(dy, 0), mask.shape[0] + min(dy, 0))
    xs = slice(max(dx, 0), mask.shape[1] + min(dx, 0))
    src = slice(max(-dy, 0), mask.shape[0] + min(-dy, 0))
    srcx = slice(max(-dx, 0), mask.shape[1] + min(-dx, 0))
    out[ys, xs] = mask[src, srcx]
    return out


def _dilate(mask: np.ndarray) -> np.ndarray:
    """One pixel of 4-neighbour dilation, without SciPy."""
    return (mask | _shift(mask, 1, 0) | _shift(mask, -1, 0)
            | _shift(mask, 0, 1) | _shift(mask, 0, -1))


def _fill_holes(mask: np.ndarray, near_border: int) -> np.ndarray:
    """Fill the enclosed knock-outs of a mask.

    Enclosed whites closer than near_border px to the canvas edge stay
    unfilled, so a grey card behind the artwork never becomes part of it.
    """
    outside = np.zeros_like(mask)
    outside[0, :] = outside[-1, :] = outside[:, 0] = outside[:, -1] = True
    outside &= ~mask
    while True:
        grown = _dilate(outside) & ~mask
        if np.array_equal(grown, outside):
            break
        outside = grown
    holes = ~outside & ~mask
    if near_border:
        edge = np.zeros_like(mask)
        edge[:near_border, :] = edge[-near_border:, :] = True
        edge[:, :near_border] = edge[:, -near_border:] = True
        holes &= ~edge
    return mask | holes


def _composite_white(im: Image.Image) -> np.ndarray:
    """Flatten alpha onto white: RGB becomes the visible colour."""
    rgb = np.asarray(im.convert("RGB"), dtype=np.float64)
    alpha = np.asarray(im.convert("RGBA"), dtype=np.float64)[:, :, 3:4] / 255.0
    return rgb * alpha + 255.0 * (1.0 - alpha)


def load_ink(path: Path) -> tuple[Image.Image, float, float]:
    """Return the heron on its white paper, plus its ink centroid.

    The artwork is black ink on a white paper whose white also carries the
    knock-out (the eye pupil, the plumage lines), and the plate the icon puts
    behind it is white too, so the bird travels as an opaque rectangle-shaped
    silhouette: the eyelet stays an eyelet instead of turning into a black
    blot, and the icon keeps its white-out-of-black reading.
    """
    rgb = _composite_white(Image.open(path))        # the artwork as it reads
    smooth = np.asarray(Image.fromarray(rgb.astype(np.uint8)).filter(
        ImageFilter.MedianFilter(DESPECKLE))).astype(np.float64)
    paper = 255.0 - rgb.min(axis=2)                 # 0 on white, 255 on ink
    ink = paper > INK_GREY
    # Pale 1-2 px hairs run across the knock-out lines (the eyelet, the light
    # edges); the solid shape traces through them so the eyelet is immune.
    solid = ink & ~((np.abs(smooth - rgb).max(axis=2) > DESPECKLE_STEP) & ink)
    # The master's alpha is a soft vignette and says nothing about the shape.
    # The paper around the bird is not part of it, so alpha is the filled
    # silhouette and the white knock-outs inside travel as white ink.
    alpha = _fill_holes(solid, near_border=DESPECKLE * 3).astype(np.float64)
    ys, xs = np.nonzero(alpha > 0)
    x0, x1, y0, y1 = xs.min(), xs.max(), ys.min(), ys.max()
    out = np.zeros((y1 - y0 + 1, x1 - x0 + 1, 4), dtype=np.uint8)
    out[:, :, :3] = rgb.astype(np.uint8)[y0:y1 + 1, x0:x1 + 1]
    out[:, :, 3] = (alpha[y0:y1 + 1, x0:x1 + 1] * 255.0).round()
    weight = solid[y0:y1 + 1, x0:x1 + 1].astype(np.float64)  # solid shape only
    cx = float((weight.sum(axis=0) * np.arange(out.shape[1])).sum() / weight.sum())
    cy = float((weight.sum(axis=1) * np.arange(out.shape[0])).sum() / weight.sum())
    return Image.fromarray(out), cx, cy


def superellipse_mask(size: int, body: int, corner: float) -> Image.Image:
    """Continuous squircle grown from signed distance fields (macOS style)."""
    ss = 4
    S = size * ss
    d = body / 2.0 * ss
    c = S / 2.0
    r = corner * d

    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    px = np.abs(xx - c) - (d - r)
    py = np.abs(yy - c) - (d - r)
    cx = np.clip(px, 0, None)
    cy = np.clip(py, 0, None)
    sdf = np.sqrt(cx ** 2 + cy ** 2) + np.minimum(np.maximum(px, py), 0) - r
    cov = np.clip(0.5 - sdf, 0, 1)
    return Image.fromarray((cov * 255).round().astype(np.uint8))


def supersample(im: Image.Image, factor: int) -> Image.Image:
    return im.resize((im.width * factor, im.height * factor), Image.LANCZOS)


def compose(size: int, body: int, corner: float, ink: Image.Image,
            centroid_x: float, plate: tuple[int, int, int] | None) -> Image.Image:
    """Render one icon master; plate=None leaves the body transparent."""
    ss = 4
    S = size * ss
    canvas = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    if plate is not None:
        body_img = Image.new("RGBA", (S, S), plate + (255,))
        canvas.paste(body_img, (0, 0), superellipse_mask(size, body, corner))

    scale = min(body * ART_MAX_W / ink.width, body * ART_MAX_H / ink.height) * ss
    art = supersample(ink, ss).resize(
        (max(1, round(ink.width * scale)), max(1, round(ink.height * scale))),
        Image.LANCZOS)

    # Center the ink mass horizontally so the long beak balances the body.
    # Center its full visible bounds vertically so the feet stay inside the plate.
    scx = centroid_x * scale
    origin_x = S / 2.0 - scx
    origin_y = S / 2.0 - art.height / 2.0 - OPTICAL_RISE * body * ss
    left, top = round(origin_x), round(origin_y)

    canvas.alpha_composite(art, (left, top))
    return canvas.resize((size, size), Image.BOX)


# --- writers ----------------------------------------------------------------
def png_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def write_icns(master: Image.Image, path: Path) -> None:
    chunks = b""
    for px, kind in ICNS_TYPES.items():
        im = master.resize((px, px), Image.LANCZOS) if px != master.width else master
        payload = png_bytes(im)
        chunks += kind + struct.pack(">I", len(payload) + 8) + payload
    path.write_bytes(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)


def write_ico(master: Image.Image, path: Path) -> None:
    frames = []
    for px in ICO_SIZES:
        frames.append((px, master.resize((px, px), Image.LANCZOS)))
    blobs = [png_bytes(im) for _, im in frames]
    header = struct.pack("<HHH", 0, 1, len(frames))
    offset = 6 + 16 * len(frames)
    entries = b""
    for (px, _), blob in zip(frames, blobs):
        dim = 0 if px >= 256 else px
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    path.write_bytes(header + entries + b"".join(blobs))


PNG_SIZES = {
    "16x16.png": 16, "24x24.png": 24, "32x32.png": 32, "48x48.png": 48,
    "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256,
    "256x256.png": 256, "512x512.png": 512,
}
MSIX = {
    "Square30x30Logo.png": 30, "Square44x44Logo.png": 44, "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89, "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142, "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284, "Square310x310Logo.png": 310, "StoreLogo.png": 50,
}


def emit_family(out: Path, master: Image.Image, windows: Image.Image) -> None:
    out.mkdir(parents=True, exist_ok=True)
    master.save(out / "icon-master.png", optimize=True)
    windows.save(out / "windows-master.png", optimize=True)
    write_icns(master, out / "icon.icns")
    write_ico(master, out / "icon.ico")
    for name, px in PNG_SIZES.items():
        master.resize((px, px), Image.LANCZOS).save(out / name, optimize=True)
    master.resize((512, 512), Image.LANCZOS).save(out / "icon.png", optimize=True)
    # Windows-side assets: full-bleed plate, no transparent margin where the OS
    # draws its own tile or frame.
    windows.resize((256, 256), Image.LANCZOS).save(out / "icon-windows.png", optimize=True)
    for name, px in MSIX.items():
        windows.resize((px, px), Image.LANCZOS).save(out / name, optimize=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default=str(ROOT / "docs/icons/white-bg-black-silhouette/icon-master.png"))
    args = ap.parse_args()

    ink, cx, cy = load_ink(Path(args.source))
    mac = compose(SIZE, MAC_BODY, MAC_SQUIRCLE, ink, cx, (255, 255, 255))
    win = compose(SIZE, WIN_BODY, WIN_SQUIRCLE, ink, cx, (255, 255, 255))

    emit_family(ROOT / "src-tauri/icons", mac, win)
    print("wrote src-tauri/icons (macOS squircle + Windows full bleed)")


if __name__ == "__main__":
    main()
