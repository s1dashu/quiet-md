"""Screenshot paste regression, using a fake clipboard and temporary libraries.

Build with --release --features devtools, then run this file. Needs Pillow.
FDL_BIN selects another build. No system clipboard or user library is accessed.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

from hlib import doc_file, docs
from PIL import Image

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-clipboard-images-"))
source = work / "source"
(source / "docs").mkdir(parents=True)
doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
(source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1}}))
(source / "preferences.json").write_text(json.dumps({
    "appearance": "light", "english": True, "seen_version": "0.1.9",
    "auto_update": False, "quick_hotkey": False,
}))
original = "<title>Screenshot paste</title>\n<p>BeforeAfter</p>\n"
(source / "docs" / f"{doc_id}.xml").write_text(original)
pixels = Image.new("RGBA", (64, 32), (30, 120, 220, 180))
pixels.save(work / "screenshot.tiff")
pixels.save(work / "screenshot.png")
(work / "text.txt").write_text("screenshot.png")
(work / "rich.html").write_text("<p><b>Rich text</b></p>")
# #252: a path in the pasteboard instead of image data (WeChat).
(work / "path.txt").write_text(f"{work / 'screenshot.png'}")
pixels.save(work / "截图 space.png")
(work / "path-url.txt").write_text((work / "截图 space.png").as_uri())
(work / "mixed.txt").write_text(f"{work / 'screenshot.png'}\nnot a path")
Image.new("RGBA", (128, 128), (210, 40, 160, 255)).save(work / "finder-icon.png")
pixels.convert("RGB").save(work / "photo.jpg", quality=92)
pixels.save(work / "animated.gif", save_all=True, append_images=[Image.new("RGBA", pixels.size, (230, 60, 50, 255))], duration=150, loop=0)
selected = set(filter(None, os.environ.get("CLIPBOARD_IMAGE_CASES", "").split(",")))


def run(name, clipboard, finish="", plain=False, expected_images=1, expected_text=None, expected_files=None):
    if selected and name not in selected:
        return
    folder = work / name
    script = work / f"{name}.txt"
    # The only document opens automatically. Select its body through
    # Find so the insertion position does not depend on font metrics.
    script.write_text(f"""resize 1200 850
wait 1200
key cmd-f
type BeforeAfter
key escape
key left
key right
key right
key right
key right
key right
key right
clip clear
{clipboard}
key {"cmd-shift-v" if plain else "cmd-v"}
wait 700
{finish}
wait 700
move 1100 700
shot result
""")
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)],
                   env=dict(os.environ, FDL_NOBLINK="1"), check=True, timeout=60)
    saved = doc_file(folder / "lib").read_text()
    doc = ET.fromstring("<doc>" + saved + "</doc>")
    images = doc.findall(".//img")
    assert len(images) == expected_images, f"{name}: expected {expected_images} images; {saved}; see {folder}"
    if images:
        paragraphs = ["".join(p.itertext()) for p in doc.findall("p")]
        assert paragraphs == ["Before", "After"], f"{name}: caret text changed: {paragraphs}"
        for i, image in enumerate(images):
            path = Path(image.attrib["src"])
            if not path.is_absolute():
                path = folder / "lib" / path
            assert path.resolve().is_relative_to((folder / "lib").resolve()), f"{name}: original was not copied into the library"
            expected = Image.open(expected_files[i]).convert("RGBA") if expected_files else pixels
            assert Image.open(path).convert("RGBA").tobytes() == expected.tobytes(), f"{name}: original pixels replaced by a clipboard icon; see {folder}"
            if expected_files:
                assert path.suffix == expected_files[i].suffix
                assert path.read_bytes() == expected_files[i].read_bytes(), f"{name}: original image bytes changed"
            else:
                assert path.suffix == ".png", f"{name}: image is not portable PNG"
    elif expected_text is not None:
        paragraphs = "".join("".join(p.itertext()) for p in doc.findall("p"))
        assert paragraphs == expected_text, f"{name}: unexpected text: {paragraphs}"
    print(f"{name}: passed", flush=True)


print(f"Artifacts: {work}", flush=True)
tiff = f"clip image {work / 'screenshot.tiff'}"
png_text = f"clip image {work / 'screenshot.png'}\nclip text {work / 'text.txt'}"
run("tiff", tiff)
run("png_with_text", png_text)
run("undo", tiff, finish="key cmd-z", expected_images=0, expected_text="BeforeAfter")
run("redo", tiff, finish="key cmd-z\nkey cmd-shift-z")
run("multiple", tiff + "\n" + png_text, expected_images=2)
run("plain", png_text, plain=True, expected_images=0, expected_text="Beforescreenshot.pngAfter")
run("rich_html", png_text + f"\nclip html {work / 'rich.html'}",
    expected_images=0, expected_text="BeforeRich textAfter")
run("path_text", f"clip text {work / 'path.txt'}")
run("path_url", f"clip text {work / 'path-url.txt'}")
run("path_mixed", f"clip text {work / 'mixed.txt'}", expected_images=0)
run("path_plain", f"clip text {work / 'path.txt'}", plain=True,
    expected_images=0, expected_text=f"Before{work / 'screenshot.png'}After")

# Finder can supply the same icon bitmap for an image file as for a video.
file_png = f"clip files {work / 'screenshot.png'}"
icon = f"clip image {work / 'finder-icon.png'}"
run("file_icon", file_png + "\n" + icon, expected_files=[work / "screenshot.png"])
run("file_only", file_png, expected_files=[work / "screenshot.png"])
run("file_html_icon", file_png + "\n" + icon + f"\nclip html {work / 'rich.html'}", expected_files=[work / "screenshot.png"])
run("file_multiple", f"clip files {work / 'screenshot.png'}|{work / '截图 space.png'}\n" + icon,
    expected_images=2, expected_files=[work / "screenshot.png", work / "截图 space.png"])
run("file_undo", file_png + "\n" + icon, finish="key cmd-z", expected_images=0, expected_text="BeforeAfter")
run("file_redo", file_png + "\n" + icon, finish="key cmd-z\nkey cmd-shift-z", expected_files=[work / "screenshot.png"])
run("file_jpeg", f"clip files {work / 'photo.jpg'}\n" + icon, expected_files=[work / "photo.jpg"])
run("file_gif", f"clip files {work / 'animated.gif'}\n" + icon, expected_files=[work / "animated.gif"])
run("file_unavailable", f"clip files {work / 'unavailable.png'}\n" + tiff)
run("file_plain", file_png + "\n" + icon + f"\nclip text {work / 'text.txt'}", plain=True,
    expected_images=0, expected_text="Beforescreenshot.pngAfter")
