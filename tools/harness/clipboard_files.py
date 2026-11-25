"""Copied videos must paste their file, even when the clipboard has an icon.

Requires a devtools release build, ffmpeg and Pillow. All clipboard input is
simulated; files and notes live in temporary directories.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from PIL import Image
from hlib import doc_file

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-clipboard-files-"))
source = work / "source"
(source / "docs").mkdir(parents=True)
doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
(source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1}}))
(source / "preferences.json").write_text(json.dumps({"theme": "craft", "appearance": "light",
    "english": True, "seen_version": "0.1.14", "auto_update": False, "quick_hotkey": False}))
(source / "docs" / (doc_id + ".xml")).write_text("<title>Copied video</title>\n<p>BeforeAfter</p>\n")
video = work / "视频 clip.mp4"
subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=red:s=640x360:d=9:r=30",
    "-c:v", "libx264", "-pix_fmt", "yuv420p", str(video)], check=True)
picture = work / "picture.png"
Image.new("RGB", (64, 32), (30, 120, 220)).save(picture)
(work / "text.txt").write_text(video.name)
(work / "rich.html").write_text("<p>Video icon fallback</p>")
attachment = work / "notes.txt"
attachment.write_text("A copied attachment\n")


def run(name, paths, bitmap=True, finish="", plain=False, expected=("figure",), text=("Before", "After")):
    script = work / (name + ".txt")
    script.write_text("resize 1200 850\nwait 1200\nkey cmd-f\ntype BeforeAfter\nkey escape\nkey left\n"
        + "key right\n" * 6 + f"clip clear\nclip files {'|'.join(map(str, paths))}\nclip text {work / 'text.txt'}\n"
        + (f"clip image {picture}\nclip html {work / 'rich.html'}\n" if bitmap else "")
        + f"key {'cmd-shift-v' if plain else 'cmd-v'}\nwait 1600\n" + finish
        + "\nkey cmd-s\nwait 700\nmove 1150 820\nshot pasted\n")
    folder = work / name
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)],
        env=dict(os.environ, FDL_NOBLINK="1"), check=True, timeout=40)
    saved = doc_file(folder / "lib")
    doc = ET.fromstring("<doc>" + saved.read_text() + "</doc>")
    assert tuple(el.tag for el in doc if el.tag in ("figure", "img")) == expected, saved.read_text()
    assert tuple("".join(el.itertext()) for el in doc.findall("p")) == text, saved.read_text()
    for el in doc.findall("figure"):
        src = el.find("source")
        path = Path(src.get("path"))
        stored = path if path.is_absolute() else folder / "lib" / path
        original = next(p for p in paths if p.name == src.get("name"))
        assert stored.read_bytes() == original.read_bytes(), "File bytes were lost"
        if src.get("name").endswith(".mp4"):
            assert el.get("view-type") == "Preview", "Video did not enter the player"
    assert "quicklook " not in (folder / "log").read_text()
    if expected == ("figure",):
        im = Image.open(folder / "pasted.png").convert("RGB")
        rgb = im.getpixel((round(im.width * 700 / 1200), round(im.height * 400 / 850)))
        assert rgb[0] > 200 and max(rgb[1:]) < 50, f"Video did not decode: {rgb}"
    print(f"PASS {name}", flush=True)


print(f"Artifacts: {work}", flush=True)
run("video_with_icon", [video])
run("file_reference_only", [video], bitmap=False)
run("mixed_files", [video, picture, attachment], expected=("figure", "img", "figure"))
run("undo", [video], finish="key cmd-z\n", expected=(), text=("BeforeAfter",))
run("redo", [video], finish="key cmd-z\nkey cmd-shift-z\n")
run("plain", [video], plain=True, expected=(), text=("Before" + video.name + "After",))
