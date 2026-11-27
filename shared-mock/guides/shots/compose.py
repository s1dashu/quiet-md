"""Put a window screenshot (an offscreen `shot`, 2x) on the site's meadow, as the README
and Getting Started pictures show it. Scripted windows never take focus, so the system
would draw grey traffic lights and a flat grey sidebar: this draws the lights and
rebuilds the glass from the backdrop (blurred, saturated, brightened, under 55% white,
multiplied over the sidebar), as marketing's films do.
usage: /usr/bin/python3 compose.py shot.png out.png [--sidebar PX] [--flat #rrggbb]
         [--canvas W H] [--at X Y] [--scale K]   (pixels; default 2400×1500 with the window at 200,135;
         --scale shrinks a wider window to the frame the others have, its lights and corner with it)
Needs Pillow (the system python3 has it)."""
import argparse, os
from PIL import Image, ImageDraw, ImageFilter, ImageChops, ImageEnhance
ap = argparse.ArgumentParser()
ap.add_argument('shot'); ap.add_argument('out')
ap.add_argument('--sidebar', type=int, default=0, help='sidebar width in pixels; 0: no glass')
ap.add_argument('--flat', help='a plain backdrop colour instead of the meadow')
ap.add_argument('--canvas', type=int, nargs=2, default=(2400, 1500)); ap.add_argument('--at', type=int, nargs=2, default=(200, 135))
ap.add_argument('--scale', type=float, default=1, help='resize the shot by this first; --sidebar is after it')
ap.add_argument('--no-lights', action='store_true', help='the shot has no title bar')
a = ap.parse_args()
repo = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..')
(W, H), (X, Y), R = a.canvas, a.at, 52  # R: macOS 26's window corner, 26pt
win = Image.open(a.shot).convert('RGB'); s = a.scale
if s != 1: win = win.resize((round(win.width * s), round(win.height * s)), Image.LANCZOS)
ww, wh = win.size; R = round(R * s)
if a.flat:
    canvas = Image.new('RGB', (W, H), a.flat)
else:  # the meadow keeps its pixels: nearest-neighbour, cover, centred
    bg = Image.open(os.path.join(repo, 'site', 'public', 'bg.png')).convert('RGB'); k = max(W / bg.width, H / bg.height)
    big = bg.resize((round(bg.width * k), round(bg.height * k)), Image.NEAREST)
    ox, oy = (big.width - W) // 2, (big.height - H) // 2
    canvas = big.crop((ox, oy, ox + W, oy + H))
if a.sidebar:
    glass = canvas.crop((X, Y, X + a.sidebar, Y + wh)).filter(ImageFilter.GaussianBlur(70))
    glass = ImageEnhance.Brightness(ImageEnhance.Color(glass).enhance(1.4)).enhance(1.15)
    glass = Image.blend(glass, Image.new('RGB', glass.size, 'white'), 0.55)
    side = win.crop((0, 0, a.sidebar, wh))
    # white surfaces over the sidebar (the formatting toolbar, a menu) stay white
    white = side.convert('L').point(lambda v: 255 if v >= 252 else 0).filter(ImageFilter.MaxFilter(3))
    win.paste(Image.composite(side, ImageChops.multiply(side, glass), white), (0, 0))
if not a.no_lights:  # 14pt lights, 22.5pt apart, centred 23.5pt down
    d = ImageDraw.Draw(win)
    for cx, fill, edge in ((52, '#ff5f57', '#e0443e'), (97, '#febc2e', '#dea123'), (142, '#28c840', '#1aab29')):
        d.ellipse(tuple(v * s for v in (cx - 14, 33, cx + 14, 61)), fill=edge); d.ellipse(tuple(v * s for v in (cx - 13, 34, cx + 13, 60)), fill=fill)
mask = Image.new('L', (ww * 4, wh * 4), 0); ImageDraw.Draw(mask).rounded_rectangle((0, 0, ww * 4 - 1, wh * 4 - 1), R * 4, fill=255)
mask = mask.resize((ww, wh), Image.LANCZOS)
shadow = Image.new('L', (W, H), 0)
for blur, dy, alpha in ((90, 50, 110), (24, 14, 60)):  # a wide soft drop and a close one
    layer = Image.new('L', (W, H), 0); ImageDraw.Draw(layer).rounded_rectangle((X, Y + dy, X + ww, Y + wh + dy), R, fill=alpha)
    shadow = ImageChops.lighter(shadow, layer.filter(ImageFilter.GaussianBlur(blur)))
canvas = Image.composite(Image.new('RGB', (W, H), (0, 22, 14)), canvas, shadow)
ring = Image.new('L', (W, H), 0); ImageDraw.Draw(ring).rounded_rectangle((X - 1, Y - 1, X + ww, Y + wh), R + 1, fill=60)
canvas = Image.composite(Image.new('RGB', (W, H), 'black'), canvas, ring)
canvas.paste(win, (X, Y), mask)
canvas.save(a.out)
