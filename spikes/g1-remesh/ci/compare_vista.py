# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare a rendered vista against a golden image, and fail loudly.

    python ci/compare_vista.py out/vista_b.png ci/golden/vista_b.png

This is the assertion the plan's §5 `geometry` job is *for*. The job was
originally "run Godot, upload the PNG as an artefact", which asserts nothing:
cracks, missing faces and inverted winding — the bug class the screenshot exists
to catch, and the one this spike actually hit — would all have gone through a
green tick. Two checks run here:

1. **Structural.** The image exists, has the expected size, and is not blank.
   `--headless` selects the dummy rendering driver, `frame_post_draw` never
   fires under it, and `main.gd`'s screenshot coroutine then never resumes, so
   "no image at all" is the most likely failure and has to be an error rather
   than an empty artefact directory.

2. **Golden compare**, with a generous tolerance: mean absolute difference per
   channel, plus the fraction of pixels that differ by more than a hard
   threshold. Generous, because the two sides may be different rasterisers —
   what must not change is *what geometry is there*, and a back-face-culled hole
   or a missing chunk moves both statistics by far more than a rasteriser
   tie-break does.

Pure standard library plus zlib: no Pillow, no numpy, so the CI job needs no
Python packaging step. Only 8-bit RGB/RGBA non-interlaced PNGs are handled,
which is what Godot's `Image.save_png` writes.
"""

import struct
import sys
import zlib


def read_png(path):
    """Return (width, height, channels, bytes) for an 8-bit non-interlaced PNG."""
    with open(path, "rb") as fh:
        data = fh.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos = 8
    width = height = None
    channels = 0
    idat = bytearray()
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        ctype = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if ctype == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(
                ">IIBBBBB", body
            )
            if depth != 8 or interlace != 0 or colour not in (2, 6):
                raise ValueError(
                    f"{path}: only 8-bit non-interlaced RGB/RGBA is handled "
                    f"(depth={depth} colour={colour} interlace={interlace})"
                )
            channels = 3 if colour == 2 else 4
        elif ctype == b"IDAT":
            idat += body
        elif ctype == b"IEND":
            break

    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    out = bytearray(height * stride)
    prev = bytearray(stride)
    p = 0
    for y in range(height):
        filt = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if filt == 1:  # Sub
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif filt == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif filt == 3:  # Average
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif filt == 4:  # Paeth
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        elif filt != 0:
            raise ValueError(f"{path}: unknown row filter {filt}")
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, channels, bytes(out)


def diff(a, b):
    """(mean abs diff per channel, fraction of pixels differing by > 32, histogram)."""
    aw, ah, ac, ad = a
    bw, bh, bc, bd = b
    if (aw, ah) != (bw, bh):
        raise ValueError(f"size mismatch: {aw}x{ah} vs {bw}x{bh}")
    total = 0
    hard = 0
    pixels = aw * ah
    hist = {}
    for i in range(pixels):
        worst = 0
        for k in range(3):
            d = abs(ad[i * ac + k] - bd[i * bc + k])
            total += d
            worst = max(worst, d)
        if worst:
            hist[worst] = hist.get(worst, 0) + 1
        if worst > 32:
            hard += 1
    return total / (pixels * 3), hard / pixels, hist


def variance_ok(img, floor=200.0):
    """A blank or single-colour frame is a failed render, not a passed test."""
    w, h, c, d = img
    n = w * h
    step = max(1, n // 100_000)
    vals = [d[i * c] for i in range(0, n, step)]
    mean = sum(vals) / len(vals)
    var = sum((v - mean) ** 2 for v in vals) / len(vals)
    return var >= floor, var


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    shot = sys.argv[1]
    golden = sys.argv[2] if len(sys.argv) > 2 else None
    max_mean = float(sys.argv[3]) if len(sys.argv) > 3 else 6.0
    max_hard = float(sys.argv[4]) if len(sys.argv) > 4 else 0.02

    try:
        img = read_png(shot)
    except FileNotFoundError:
        print(
            f"FAIL: {shot} was not produced at all. Godot rendered nothing — "
            "--headless uses the dummy driver and cannot take a screenshot; run "
            "windowed under a software rasteriser.",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"{shot}: {img[0]}x{img[1]}, {img[2]} channels")

    ok, var = variance_ok(img)
    print(f"  red-channel variance {var:.1f}")
    if not ok:
        print("FAIL: the vista is blank or near-uniform; nothing was drawn", file=sys.stderr)
        sys.exit(1)

    if golden is None:
        print("no golden given: structural check only")
        return
    try:
        gold = read_png(golden)
    except FileNotFoundError:
        print(
            f"FAIL: golden image {golden} is missing. Bootstrap it by taking the "
            "artefact this job uploaded, eyeballing it for cracks, missing faces "
            "and inverted winding, and committing it (force-added past "
            ".gitignore) — the job must not pass with nothing to compare against.",
            file=sys.stderr,
        )
        sys.exit(1)

    if (img[0], img[1]) != (gold[0], gold[1]):
        print(
            f"FAIL: {shot} is {img[0]}x{img[1]} but the golden is "
            f"{gold[0]}x{gold[1]}. The vista must be rendered at the golden's "
            "resolution or the comparison is meaningless — check --resolution, "
            "and note that a windowed run is clamped by the desktop/xvfb screen "
            "size (a 1920x1080 request on a 1920x1080 screen yields a 1920x1061 "
            "client area).",
            file=sys.stderr,
        )
        sys.exit(1)

    mean, hard, hist = diff(img, gold)
    worst = max(hist) if hist else 0
    differing = sum(hist.values())
    print(
        f"  vs {golden}: mean abs diff {mean:.4f}/255, "
        f"{differing} of {img[0] * img[1]} pixels differ "
        f"({100.0 * differing / (img[0] * img[1]):.2f}%), worst channel delta {worst}, "
        f"{100.0 * hard:.3f}% differ by more than 32"
    )
    if mean > max_mean or hard > max_hard:
        print(
            f"FAIL: geometry differs from the golden (mean {mean:.3f} > {max_mean} "
            f"or {hard:.4f} > {max_hard}). Look for cracks, missing faces or "
            "inverted winding before regenerating the golden.",
            file=sys.stderr,
        )
        sys.exit(1)
    print("geometry ok")


if __name__ == "__main__":
    main()
