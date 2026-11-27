"""A full-width comma typed into a Latin line leaves the line where it was
(report of 2026-11-27): the line's baseline follows the text's own font, as a
CSS strut does under a fixed line-height, not the largest of every run's font
(the comma is set in `CJK_PUNCT_FONT`, PingFang SC on a Mac, whose ascent and
descent are larger). Before the fix the line's Latin letters moved 0.3-0.5pt,
up in SF and down in Times New Roman, as the comma came and went.

A paragraph and a heading in Craft (SF), a paragraph in Feishu with Times New
Roman as the document font: the caret at the end of a Latin line, shot; "，"
typed and the caret back before it, shot again. Only the comma may differ.

Build with --release --features devtools, then run this file (FDL_BIN selects
another build). Libraries and shots stay in the printed temporary folder; it
exits 1 when a line moved.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image, ImageChops

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-cjk-baseline-"))
doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
CASES = [("craft-p", "craft", "", "<p>Hxgk Hxgk</p>"), ("craft-h1", "craft", "", "<h1>Hxgk Hxgk</h1>"),
         ("feishu-times", "feishu", "Times New Roman", "<p>Hxgk Hxgk</p>")]


def run(name, theme, family, block):
    source = work / (name + "-source")
    (source / "docs").mkdir(parents=True)
    (source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1}}))
    (source / "preferences.json").write_text(json.dumps({
        "theme": theme, "feishu": theme == "feishu", "doc_family": family, "appearance": "light", "english": True,
        "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False}))
    (source / "docs" / f"{doc_id}.xml").write_text(f"<title>Baseline</title>\n<p>Text above</p>\n{block}\n")
    script = work / (name + ".txt")
    script.write_text("resize 1000 700\nwait 1500\nmove 990 690\nkey cmd-down\nkey end\nwait 300\nshot a\n"
                      "type ，\nkey left\nwait 300\nshot b\nquit\n")
    folder = work / name
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)],
                   env=dict(os.environ, FDL_NOBLINK="1"), check=True, timeout=120)
    return folder


failed = []
for name, theme, family, block in CASES:
    f = run(name, theme, family, block)
    a, b = (Image.open(f / n).convert("L") for n in ("a.png", "b.png"))
    box = ImageChops.difference(a, b).getbbox()
    if not box:
        failed.append(f"{name}: the comma didn't show")
        continue
    # The line's ink from the note's left edge: the first letters must not have moved.
    px = a.load()
    left = min((x for x in range(a.width * 3 // 10, box[0] + 1) for y in range(box[1], box[3]) if px[x, y] < 140), default=box[0])
    print(f"{name}: changed {box}, line starts at x={left}")
    if box[0] < left + 20:
        failed.append(f"{name}: the Latin letters moved (changed {box}, text from x={left})")
print(work)
if failed:
    sys.exit("\n".join(failed))
