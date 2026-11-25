"""macOS inline video regression, with generated media and a throwaway library.

Build --release --features devtools, then run with Python + Pillow and ffmpeg.
Checks real decoded pixels, paused frames, seeking, mute, replay, card clicks,
portrait orientation and unreadable media. Never launches Quick Look.
"""
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile

from PIL import Image, ImageChops

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-video-playback-"))
source = work / "source"
(source / "docs").mkdir(parents=True)
(source / "assets").mkdir()
doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
(source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1}}))
prefs = {"theme": "craft", "english": True, "seen_version": "0.1.14", "auto_update": False,
         "quick_hotkey": False, "remote_images": False}

subprocess.run(["ffmpeg", "-v", "error", "-y",
    "-f", "lavfi", "-i", "color=red:s=640x360:d=3:r=30",
    "-f", "lavfi", "-i", "color=blue:s=640x360:d=3:r=30",
    "-f", "lavfi", "-i", "color=lime:s=640x360:d=3:r=30",
    "-filter_complex", "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]", "-map", "[v]",
    "-c:v", "libx264", "-pix_fmt", "yuv420p", str(source / "assets/sample.mp4")], check=True)
subprocess.run(["ffmpeg", "-v", "error", "-y", "-display_rotation", "90", "-i", str(source / "assets/sample.mp4"),
    "-c", "copy", str(source / "assets/portrait.mov")], check=True)
subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=red:s=640x360:d=2:r=30",
    "-c:v", "libx264", "-pix_fmt", "yuv420p", str(source / "assets/short.mp4")], check=True)
(source / "assets/broken.mp4").write_bytes(b"not a video")


def run(name, steps, theme="light", view="Preview", media="sample.mp4"):
    (source / "preferences.json").write_text(json.dumps(dict(prefs, appearance=theme)))
    (source / "docs" / (doc_id + ".xml")).write_text(
        f'<title>Video playback regression</title>\n<figure view-type="{view}">'
        f'<source path="assets/{media}" name="{media}"/></figure>\n<p>After video</p>')
    script = work / (name + ".txt")
    script.write_text("resize 1200 850\nwait 2000\nmove 1150 820\n" + steps)
    folder = work / name
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)],
                   env=dict(os.environ, FDL_NOBLINK="1"), check=True, timeout=50)
    assert "quicklook " not in (folder / "log").read_text(), f"External player opened: {folder}"
    return folder


def frame(folder, name):
    return Image.open(folder / (name + ".png")).convert("RGB")


def crop(im, box):
    sx, sy = im.width / 1200, im.height / 850
    return im.crop(tuple(round(v * (sx if i % 2 == 0 else sy)) for i, v in enumerate(box)))


def color(folder, name, channel):
    im = frame(folder, name)
    rgb = im.getpixel((round(im.width * 700 / 1200), round(im.height * 400 / 850)))
    assert rgb[channel] > 200 and all(v < 50 for i, v in enumerate(rgb) if i != channel), (folder, name, rgb)


def changed(folder, before, after, box):
    a, b = crop(frame(folder, before), box), crop(frame(folder, after), box)
    assert ImageChops.difference(a, b).getbbox(), (folder, before, after, "control did not change")


for theme in os.environ.get("VIDEO_THEMES", "light,dark").split(","):
    folder = run(theme, """shot loaded
anchor video-inline-back
wait 250
anchor video-inline-forward
move 1150 820
wait 1000
shot forward
anchor video-inline-back
wait 500
anchor video-inline-progress
wait 500
anchor video-inline-back
move 1150 820
wait 1000
shot near-start
anchor video-inline-back
move 1150 820
wait 1000
shot beginning
anchor video-inline-progress
wait 500
anchor video-inline-forward
move 1150 820
wait 1000
shot near-end
anchor video-inline-forward
move 1150 820
wait 1000
shot end
anchor video-inline-forward
move 1150 820
wait 300
shot end-held
anchor video-inline-back
anchor video-inline-back
anchor video-inline-back
move 1150 820
wait 1000
shot rapid-start
anchor video-inline-forward
anchor video-inline-forward
anchor video-inline-forward
move 1150 820
wait 1000
shot rapid-end
anchor video-inline-back
anchor video-inline-back
anchor video-inline-back
wait 500
anchor video-inline-center-play
move 1150 820
wait 3500
shot playing-hidden
move 700 400
wait 150
shot playing-hover
anchor video-inline-mute
wait 150
shot playing-muted
anchor video-inline-mute
wait 100
anchor video-inline-play
move 1150 820
wait 200
shot paused
wait 1200
shot held
anchor video-inline-progress
wait 500
drag 700 590 500 590
move 1150 820
wait 1000
shot dragged
anchor video-inline-mute
move 1150 820
shot mute
anchor video-inline-forward
anchor video-inline-forward
anchor video-inline-forward
wait 500
anchor video-inline-center-play
move 1150 820
wait 500
shot replay
""", theme=theme)
    for shot, channel in [("loaded", 0), ("forward", 2), ("near-start", 0), ("beginning", 0),
            ("near-end", 1), ("end", 1), ("rapid-start", 0), ("rapid-end", 1),
            ("playing-hidden", 2), ("dragged", 0), ("replay", 0)]:
        color(folder, shot, channel)
    assert ImageChops.difference(frame(folder, "paused"), frame(folder, "held")).getbbox() is None, f"Pause did not hold: {folder}"
    changed(folder, "playing-hidden", "playing-hover", (370, 575, 1115, 605))
    changed(folder, "playing-hidden", "paused", (720, 380, 760, 420))
    changed(folder, "playing-hover", "playing-muted", (1090, 575, 1120, 605))
    pause_box = (362, 578, 386, 602)
    assert ImageChops.difference(crop(frame(folder, "playing-hover"), pause_box), crop(frame(folder, "playing-muted"), pause_box)).getbbox() is None, "Moving onto the controls interrupted playback"
    changed(folder, "dragged", "mute", (1090, 575, 1120, 605))
    clock_box = (1045, 630, 1130, 658)
    assert ImageChops.difference(crop(frame(folder, "end"), clock_box), crop(frame(folder, "end-held"), clock_box)).getbbox() is None, "Forward went past the end"
    positions = {part: (int(x), int(y)) for part, x, y in re.findall(r'anchor "video-inline-([^"]+)" at (\d+) (\d+)', (folder / "log").read_text())}
    assert positions["back"][0] < positions["progress"][0] < positions["forward"][0], "Skip buttons do not flank the progress bar"
    assert all(179 < positions[part][1] < 622 for part in ("back", "forward", "progress", "play", "mute")), "Controls are outside the video"
    assert abs(positions["center-play"][0] - 740) <= 1 and abs(positions["center-play"][1] - 400) <= 1, "Paused play button is not centered"
    print(f"PASS {theme}: overlay hover, centered play, three-second skips, boundaries, rapid clicks, seeking, mute and replay", flush=True)

    fullscreen = run(theme + "-fullscreen", """anchor file-fullscreen
anchor video-full-center-play
move 1150 820
wait 3500
shot fullscreen-playing
move 700 400
wait 150
shot fullscreen-hover
anchor video-full-mute
wait 150
shot fullscreen-playing-muted
anchor video-full-mute
wait 100
key space
move 1150 820
wait 200
shot fullscreen-paused
wait 1000
shot fullscreen-held
anchor video-full-progress
wait 500
anchor video-full-back
move 1150 820
wait 1000
shot fullscreen-back
anchor video-full-back
move 1150 820
wait 500
anchor video-full-progress
wait 500
anchor video-full-forward
move 1150 820
wait 1000
shot fullscreen-forward
anchor video-full-forward
move 1150 820
wait 1000
shot fullscreen-end
anchor video-full-mute
move 1150 820
shot fullscreen-mute
key escape
wait 300
shot inline-return
anchor file-fullscreen
wait 300
resize 1000 700
wait 300
shot resized
anchor video-close
wait 300
shot closed
""", theme=theme)
    for shot, channel in [("fullscreen-playing", 2), ("fullscreen-back", 0), ("fullscreen-forward", 1), ("fullscreen-end", 1), ("inline-return", 1)]:
        color(fullscreen, shot, channel)
    assert ImageChops.difference(frame(fullscreen, "fullscreen-paused"), frame(fullscreen, "fullscreen-held")).getbbox() is None, "Fullscreen pause did not hold"
    changed(fullscreen, "fullscreen-playing", "fullscreen-hover", (40, 740, 1160, 775))
    changed(fullscreen, "fullscreen-hover", "fullscreen-playing-muted", (1140, 742, 1170, 774))
    pause_box = (32, 746, 56, 770)
    assert ImageChops.difference(crop(frame(fullscreen, "fullscreen-hover"), pause_box), crop(frame(fullscreen, "fullscreen-playing-muted"), pause_box)).getbbox() is None, "Fullscreen controls interrupted playback"
    changed(fullscreen, "fullscreen-end", "fullscreen-mute", (1140, 742, 1170, 774))
    for before, after in [("fullscreen-playing", "inline-return"), ("resized", "closed")]:
        a, b = crop(frame(fullscreen, before), (30, 300, 50, 320)), crop(frame(fullscreen, after), (30, 300, 50, 320))
        assert ImageChops.difference(a, b).getbbox(), "Fullscreen did not cover or restore the sidebar"
    print(f"PASS {theme}: fullscreen overlay, centered play, skips, mute, resize, Escape and Close", flush=True)

short = run("short", """shot beginning
anchor video-inline-forward
move 1150 820
wait 1000
shot end
anchor video-inline-forward
move 1150 820
wait 300
shot end-held
anchor video-inline-back
move 1150 820
wait 1000
shot start
""", media="short.mp4")
clock_box = (1045, 630, 1130, 658)
changed(short, "beginning", "end", clock_box)
assert ImageChops.difference(crop(frame(short, "end"), clock_box), crop(frame(short, "end-held"), clock_box)).getbbox() is None
assert ImageChops.difference(crop(frame(short, "beginning"), clock_box), crop(frame(short, "start"), clock_box)).getbbox() is None
print("PASS clips shorter than three seconds: skips stop at either boundary", flush=True)

folder = run("card", "click 528 215\nmove 1150 820\nwait 700\nshot opened\n", view="Card")
color(folder, "opened", 0)
print("PASS card: play opens inline")

folder = run("portrait", "shot portrait\n", media="portrait.mov")
im = frame(folder, "portrait")
# A rotated 640×360 video is letterboxed horizontally in the 788×600 area.
assert max(crop(im, (350, 300, 360, 310)).getpixel((0, 0))) < 30, "Portrait frame was stretched or not rotated"
color(folder, "portrait", 0)
print("PASS portrait: display transform respected")

folder = run("broken", "shot failed\nwait 1000\nshot held\nclick 700 400\nwait 300\nshot clicked\n", media="broken.mp4")
assert ImageChops.difference(frame(folder, "failed"), frame(folder, "held")).getbbox() is None, "Failed video kept loading"
assert max(crop(frame(folder, "failed"), (700, 300, 710, 310)).getpixel((0, 0))) < 30
print("PASS unreadable media: stable failure instead of an endless spinner")
print(f"Artifacts: {work}")
