"""A folder of folders full of photo notes, for folder-card and image tests.

usage: python3 tools/perf/gen_gallery.py <library dir> [folders=24] [per=4] [loose=30] [size=3024x2016]
Adds `Gallery/` to a library (a vault from gen_library.py, say): `folders`
folders of `per` Markdown notes each, every note with a heading, a paragraph
and a photo file of its own, plus `loose` such notes beside the folders.
Folder cards on the `Gallery` page then carry real first pages with photos.
Each photo is a different picture (a gradient and a pattern seeded by its
number), so each is decoded on its own and none compress alike. Needs Pillow.
"""
import os, random, sys
from PIL import Image, ImageDraw

lib = os.path.abspath(sys.argv[1])
folders = int(sys.argv[2]) if len(sys.argv) > 2 else 24
per = int(sys.argv[3]) if len(sys.argv) > 3 else 4
loose = int(sys.argv[4]) if len(sys.argv) > 4 else 30
w, h = (int(v) for v in (sys.argv[5] if len(sys.argv) > 5 else '3024x2016').split('x'))
root = os.path.join(lib, 'Gallery')

def photo(path, n):
    r = random.Random(n)
    a, b = [tuple(r.randrange(40, 230) for _ in range(3)) for _ in range(2)]
    small = Image.new('RGB', (w // 8, h // 8))
    d = ImageDraw.Draw(small)
    for y in range(h // 8):
        t = y / (h // 8)
        d.line([(0, y), (w // 8, y)], fill=tuple(int(a[i] * (1 - t) + b[i] * t) for i in range(3)))
    for _ in range(40):
        x, y, s = r.randrange(w // 8), r.randrange(h // 8), r.randrange(8, 60)
        d.ellipse([x, y, x + s, y + s], fill=tuple(r.randrange(256) for _ in range(3)))
    img = small.resize((w, h), Image.BICUBIC)
    # Grain, so the JPEG is the size of a camera's rather than a flat gradient's.
    noise = Image.effect_noise((w, h), 24).convert('RGB')
    Image.blend(img, noise, 0.12).save(path, quality=85)

def note(folder, title, n):
    os.makedirs(os.path.join(folder, 'img'), exist_ok=True)
    photo(os.path.join(folder, 'img', f'p{n}.jpg'), n)
    with open(os.path.join(folder, f'{title}.md'), 'w') as f:
        f.write(f'# {title}\n\nA trip, a dinner, a whiteboard: one photo in a short note, number {n}.\n\n![photo {n}](img/p{n}.jpg)\n\nAfterwards, a few words on what happened next.\n')

# The loose notes first: the albums, written last, come first on the page (newest first).
n = 0
for j in range(loose):
    note(root, f'Snapshot {j + 1}', n); n += 1
for i in range(folders):
    folder = os.path.join(root, f'Album {i + 1:02}')
    for j in range(per):
        note(folder, f'Day {j + 1} of album {i + 1}', n); n += 1
print(f'{n} photos in {root}')
