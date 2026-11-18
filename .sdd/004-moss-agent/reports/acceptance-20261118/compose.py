"""Tile each spec's renders into one labelled contact sheet.

usage: uv run --with pillow python compose.py <renders dir> <sheets dir> [max_width]
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

src, dst = sys.argv[1], sys.argv[2]
max_w = int(sys.argv[3]) if len(sys.argv) > 3 else 2400
os.makedirs(dst, exist_ok=True)
font = ImageFont.load_default(size=22)
PAD, LABEL = 16, 34

for spec in sorted(os.listdir(src)):
    folder = os.path.join(src, spec)
    if not os.path.isdir(folder):
        continue
    names = sorted(n for n in os.listdir(folder) if n.endswith(".png"))
    if not names:
        continue
    images = [(n[:-4], Image.open(os.path.join(folder, n)).convert("RGB")) for n in names]
    # Renders are @2x; halve them so a sheet stays readable at review size.
    images = [(n, im.resize((im.width // 2, im.height // 2), Image.LANCZOS)) for n, im in images]
    rows, row, x = [], [], PAD
    for item in images:
        w = item[1].width + PAD
        if row and x + w > max_w:
            rows.append(row)
            row, x = [], PAD
        row.append(item)
        x += w
    rows.append(row)
    width = max(PAD + sum(im.width + PAD for _, im in r) for r in rows)
    height = PAD + sum(LABEL + max(im.height for _, im in r) + PAD for r in rows)
    sheet = Image.new("RGB", (width, height), (245, 245, 245))
    draw = ImageDraw.Draw(sheet)
    y = PAD
    for r in rows:
        x = PAD
        for name, im in r:
            draw.text((x, y + 4), name, fill=(20, 20, 20), font=font)
            sheet.paste(im, (x, y + LABEL))
            x += im.width + PAD
        y += LABEL + max(im.height for _, im in r) + PAD
    out = os.path.join(dst, f"{spec}.png")
    sheet = sheet.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    sheet.save(out, optimize=True)
    print(f"{spec}: {len(images)} renders -> {out} {sheet.width}x{sheet.height}")
