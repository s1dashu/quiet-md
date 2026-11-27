"""⌘Z right after 新建子文档 / 新建文档 undoes making the page (the report of 2026-11-28), in a scratch library.

The new page opens with the keys in it, where ⌘Z had nothing to undo: making it could not be undone
(docs/FORMAT.md: one step of the file-operation history, the document moved back out and the empty
folder removed). Here, for 新建子文档 from a sidebar row's menu and from the document's 「…」, under a
document without child pages (`计划.xml` → `计划/计划.xml`) and one with them (`周报/`), and for 新建文档
(⌘N): ⌘Z with the keys in the new page, after a click on the sidebar's blank (the keys stay in the
page) and after a row's menu closed with Esc (the keys on the sidebar) undoes making it; ⌘⇧Z makes it
again. A page typed into keeps its ⌘Z: the typing goes, the page stays, also when ⌘Z is pressed again
with its typing spent.

Build with --release --features devtools, then run this file. FDL_BIN selects another build;
SUBDOC_UNDO_SHOTS a folder to copy the screenshots to. Pass case names to run only those.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-subdoc-undo-"))
source = work / "source"
(source / ".quiet").mkdir(parents=True)
(source / ".quiet" / "preferences.json").write_text(json.dumps({"appearance": "light", "english": False, "language": "zh-CN", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False}))
(source / "周报").mkdir()
(source / "计划.xml").write_text("<title>计划</title>\n<p>正文</p>\n")
(source / "周报" / "周报.xml").write_text("<title>周报</title>\n<p>周报正文</p>\n")
(source / "周报" / "第一周.xml").write_text("<title>第一周</title>\n<p>一</p>\n")
SHOTS = os.environ.get("SUBDOC_UNDO_SHOTS")
BEFORE = ["周报/周报.xml", "周报/第一周.xml", "计划.xml"]


def run(name, commands):
    folder = work / name
    script = work / f"{name}.txt"
    script.write_text(f"resize 1300 850\nwait 1500\nmove 1200 800\n{commands}\nwait 600\nmove 1200 800\nshot end\n")
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)], env=dict(os.environ, FDL_NOBLINK="1"), check=True, timeout=180)
    if SHOTS:
        Path(SHOTS).mkdir(parents=True, exist_ok=True)
        for png in folder.glob("*.png"): shutil.copy(png, Path(SHOTS) / f"{name}-{png.name}")
    return folder / "lib"


def pages(lib):
    """The pages of the library, skipping `.quiet` (the 回收站, versions)."""
    return sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml") if not p.relative_to(lib).parts[0].startswith("."))


MORE = "1276 24"  # the document's 「…」 in a 1300-wide window
NEW = {
    "row-first": ("rrow 计划\nwait 500\nrow 新建子文档\n", ["周报/周报.xml", "周报/第一周.xml", "计划/未命名文档.xml", "计划/计划.xml"]),
    "row-existing": ("rrow 周报\nwait 500\nrow 新建子文档\n", ["周报/周报.xml", "周报/未命名文档.xml", "周报/第一周.xml", "计划.xml"]),
    "doc-first": (f"row 计划\nwait 1200\nclick {MORE}\nwait 500\nrow 新建子文档\n", ["周报/周报.xml", "周报/第一周.xml", "计划/未命名文档.xml", "计划/计划.xml"]),
    "doc-existing": (f"row 周报\nwait 1200\nclick {MORE}\nwait 500\nrow 新建子文档\n", ["周报/周报.xml", "周报/未命名文档.xml", "周报/第一周.xml", "计划.xml"]),
    "new-doc": ("row 计划\nwait 1200\nkey cmd-n\n", ["周报/周报.xml", "周报/第一周.xml", "未命名文档.xml", "计划.xml"]),
}
KEYS = {"page": "", "blank": "click 140 600\nwait 300\n", "sidebar": "rrow 周报\nwait 400\nkey escape\nwait 300\n"}
picked = sys.argv[1:]
for case, (make, made) in NEW.items():
    if picked and case not in picked: continue
    make += "wait 1500\nmove 1200 800\nshot made\n"
    lib = run(f"{case}-made", make)
    assert pages(lib) == made, (case, pages(lib))
    for keys, focus in KEYS.items():
        lib = run(f"{case}-{keys}", make + focus + "key cmd-z\nwait 2500\nmove 1200 800\nshot undone\n")
        assert pages(lib) == BEFORE, (case, keys, pages(lib))
    lib = run(f"{case}-redo", make + "key cmd-z\nwait 2500\nkey cmd-shift-z\nwait 2500\n")
    assert pages(lib) == made, (case, "redo", pages(lib))
    # typed into: ⌘Z is the page's, its typing spent or not
    lib = run(f"{case}-typed", make + "type 标题\nwait 1500\nkey cmd-z\nwait 800\nkey cmd-z\nwait 800\nkey cmd-z\nwait 2500\n")
    assert len(pages(lib)) == len(made), (case, "typed", pages(lib))
    print(case, "ok", flush=True)

print("subdoc_undo: all passed", work)
