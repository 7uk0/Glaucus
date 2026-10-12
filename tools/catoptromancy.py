"""Catoptromancy: divination by mirrors. Compare our map renders with the
official ones, pixel by pixel.

    python3 -I tools/catoptromancy.py OURS_DIR REF_DIR            # score all
    python3 -I tools/catoptromancy.py OURS_DIR REF_DIR NAME [SCALE]   # write NAME.diff.png

The score is the share of pixels whose difference exceeds 40/255 after the
reference is composited on black (MapRenderer output is transparent).
Known, expected differences (docs/akasha/systems/rendering.md): official
renders are unpowered, MapRenderer ignores layer scale/offset and sorts
decals unstably, random spawners differ, and some maps were redesigned.
The diff image is ours | official | ours with differing pixels in red.
"""
import os
import sys

from PIL import Image, ImageChops, ImageStat


def load_pair(ours_path, ref_path):
    a = Image.open(ours_path).convert('RGB')
    r = Image.open(ref_path).convert('RGBA')
    bg = Image.new('RGBA', r.size, (0, 0, 0, 255))
    bg.alpha_composite(r)
    return a, bg.convert('RGB')


def score(a, b):
    d = ImageChops.difference(a, b).convert('L')
    hist = d.histogram()
    big = sum(hist[41:]) / (d.width * d.height)
    return ImageStat.Stat(d).mean[0], big


def diff_image(a, b, out, scale):
    d = ImageChops.difference(a, b).convert('L').point(lambda v: 255 if v > 40 else 0)
    over = Image.composite(Image.new('RGB', a.size, (255, 0, 0)), a.point(lambda v: v // 2), d)
    w = a.width
    canvas = Image.new('RGB', (w * 3 + 20, a.height), (40, 40, 40))
    canvas.paste(a, (0, 0))
    canvas.paste(b, (w + 10, 0))
    canvas.paste(over, (2 * w + 20, 0))
    canvas.resize((int(canvas.width * scale), int(canvas.height * scale)), Image.NEAREST).save(out)


def main():
    ours_dir, ref_dir = sys.argv[1], sys.argv[2]
    if len(sys.argv) > 3:
        name = sys.argv[3]
        scale = float(sys.argv[4]) if len(sys.argv) > 4 else 1.0
        a, b = load_pair(os.path.join(ours_dir, name + '.png'), os.path.join(ref_dir, name + '.png'))
        out = os.path.join(ours_dir, name + '.diff.png')
        diff_image(a, b, out, scale)
        print(out)
        return
    rows, mismatched = [], []
    for f in sorted(os.listdir(ours_dir)):
        if not f.endswith('.png') or f.endswith('.diff.png'):
            continue
        ref = os.path.join(ref_dir, f)
        if not os.path.exists(ref):
            continue
        a, b = load_pair(os.path.join(ours_dir, f), ref)
        if a.size != b.size:
            mismatched.append((f[:-4], a.size, b.size))
            continue
        rows.append((f[:-4], *score(a, b), a.size))
    rows.sort(key=lambda r: -r[2])
    print(f"{len(rows) + len(mismatched)} compared, {len(mismatched)} size mismatches")
    for name, mean, big, size in rows:
        print(f"{name:20s} mean {mean:5.1f}  >40: {big * 100:5.1f}%  {size}")
    for name, a, b in mismatched:
        print(f"{name:20s} size {a} vs official {b}")


if __name__ == '__main__':
    main()
