"""A page or folder moved into another top-level folder (crates/core/src/across.rs).

Build with --release --features devtools, then run this file. Two top-level
folders, One and Two (`QUIET_MD_FOLDERS`); every write is held back
(`FDL_TREE_WRITE_DELAY`) so each case also sees the rows before the files move:

  sidebar_folder  Project dragged onto Two's heading: under Two at once, its files in Two once
                  written; its picture copied into Two's .assets, its mention of a page left in
                  One (and that page's of it) written by id; ⌘Z brings it back into One
  sidebar_doc     a page dragged onto a folder of Two
  move_dialog     移动到 a page into a folder of Two, picked in the dialog's tree
  move_to_top     移动到 opened in Two: titled from Two, the top-level folders the tree's first rows
                  (no 我的文档 above them); a folder of Two moved to One's top level
  move_lone       with One the only top-level folder: a row stands in for it, and picking it moves a page to its top level
  open_page       the page open in the editor goes with its folder and is typed into: saved in Two
  fails           a folder of Two that cannot be written to: the row is back in One, nothing moved

Everything happens in a temporary library; no system clipboard or user
library is accessed. Pass case names to run only those. Screenshots of each
case are left in the artifacts folder.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-cross-top-move-"))
PREFS = json.dumps({"appearance": "light", "english": True, "language": "en", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False})
PNG = bytes.fromhex("89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c4944415408d763f8cfc000000301010018dd8db00000000049454e44ae426082")


def library(src):
    """One: a folder with a page (a picture, a mention of a page left in One, which mentions it
    back), a page at its top. Two: a folder with a page."""
    for name in ("One", "Two"):
        (src / name / ".quiet").mkdir(parents=True)
        (src / name / ".quiet" / "preferences.json").write_text(PREFS)
    (src / "One" / "Project").mkdir()
    (src / "One" / ".assets").mkdir()
    (src / "One" / ".assets" / "p.png").write_bytes(PNG)
    (src / "One" / "Project" / "Alpha.xml").write_text('<title>Alpha</title>\n<p>a</p>\n<img src=".assets/p.png"/>\n<p>See <cite type="doc" href="../Outside.xml" file-type="docx">Outside</cite></p>\n')
    (src / "One" / "Outside.xml").write_text('<title>Outside</title>\n<p>To <cite type="doc" href="Project/Alpha.xml" file-type="docx">Alpha</cite></p>\n')
    (src / "One" / "Loose.xml").write_text("<title>Loose</title>\n<p>l</p>\n")
    (src / "Two" / "Target").mkdir()
    (src / "Two" / "Target" / "Keep.xml").write_text("<title>Keep</title>\n<p>k</p>\n")
    # Newest first in the sidebar: One's Loose, Outside, Project; then Two's Target.
    for i, p in enumerate(["Two/Target/Keep.xml", "Two/Target", "One/Project/Alpha.xml", "One/Project", "One/Outside.xml", "One/Loose.xml"]):
        os.utime(src / p, (1_700_000_000 + i * 100,) * 2)
    return src


def run(name, script, locked=None, tops=("One", "Two")):
    """The script on a fresh copy of `library`, with folder `locked` (relative to the copy) read-only meanwhile,
    and `tops` the top-level folders open."""
    out = work / name
    out.mkdir(parents=True)
    lib = out / "lib"
    shutil.copytree(library(work / f"src-{name}"), lib)
    folders = out / "folders.json"
    folders.write_text(json.dumps({"v": 1, "folders": [{"key": f"k-{t.lower()}", "path": str(lib / t), "added": i + 1} for i, t in enumerate(tops)]}))
    (out / "s.txt").write_text("resize 1300 850\nwait 300\nmove 700 500\nwait 1200\n" + script + "\nquit\n")
    binary = os.environ.get("FDL_BIN") or str(here.parent.parent / "target" / "release" / "quiet_md")
    env = {"HOME": os.path.expanduser("~"), "QUIET_MD_HOME": str(lib), "QUIET_MD_FOLDERS": str(folders), "FDL_SCRIPT": str(out / "s.txt"), "FDL_SHOTS": str(out),
           "PATH": "/usr/bin:/bin", "FDL_NOBLINK": "1", "FDL_TREE_WRITE_DELAY": "1500"}
    if "TMPDIR" in os.environ: env["TMPDIR"] = os.environ["TMPDIR"]
    if locked: os.chmod(lib / locked, 0o555)
    try:
        code = subprocess.run(binary, env=env, cwd=out, stdout=open(out / "log", "w"), stderr=subprocess.STDOUT, timeout=180).returncode
    finally:
        if locked: os.chmod(lib / locked, 0o755)
    return code, lib, (out / "log").read_text(errors="replace")


def ids(lib, top):
    """The pages top-level folder `top` keeps a record of, by path: their ids (FORMAT 6 shards, each field `[value, stamp]`)."""
    out = {}
    for f in (lib / top / ".quiet" / "meta").rglob("*.json"):
        for i, fields in json.loads(f.read_text()).get("items", {}).items():
            path = (fields.get("path") or [None])[0]
            if isinstance(path, str): out[path] = i
    return out


# The sidebar as every run starts it (1300×850): One's heading, then Loose, Outside and Project
# (y 90, 121, 152), Two's heading (195), shut. Project, the last, carried down leaves the rows
# where they are: Two's heading stays at 195. Loose, carried from the top, lets the rows under it
# close up as it goes; with Two opened first, Target is then at 226.
def drag(y, *path):
    """Press on the sidebar row at `y`, carry it through the points `path` (y, ms held) and let go at the last."""
    s = f"down 70 {y}\nwait 50\ndragto 70 {y + 12}\nwait 50\n"
    for to, ms in path: s += f"dragto 64 {to}\nwait {ms}\n"
    last = path[-1][0]
    return s + f"shot over\nup 64 {last}\nwait 200\nshot ahead\n"


PROJECT_TO_TWO = drag(152, (175, 50), (195, 400))
LOOSE_TO_TARGET = "hoverrow Two\nwait 300\nclick 59 195\nwait 600\n" + drag(90, (110, 400), (150, 50), (211, 50), (226, 500))


def sidebar_folder():
    code, lib, log = run("sidebar_folder", PROJECT_TO_TWO + "wait 3000\nshot landed\n")
    assert code == 0, log[-600:]
    assert (lib / "Two" / "Project" / "Alpha.xml").is_file() and not (lib / "One" / "Project").exists(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))
    alpha = (lib / "Two" / "Project" / "Alpha.xml").read_text()
    # Its mention of a page left in One: by id now; the picture is Two's own.
    assert 'doc-id="' in alpha and 'href="../Outside.xml"' not in alpha and 'src=".assets/p.png"' in alpha, alpha
    assert (lib / "Two" / ".assets" / "p.png").read_bytes() == PNG and (lib / "One" / ".assets" / "p.png").is_file()
    one, two = ids(lib, "One"), ids(lib, "Two")
    assert "Project/Alpha.xml" in two and "Project/Alpha.xml" not in one, (one, two)
    assert f'doc-id="{two["Project/Alpha.xml"]}"' in (lib / "One" / "Outside.xml").read_text(), (lib / "One" / "Outside.xml").read_text()
    # ⌘Z: back in One, under the same id.
    code, lib, log = run("sidebar_folder_undone", PROJECT_TO_TWO + "wait 3000\nkey cmd-z\nwait 3500\nshot undone\n")
    assert code == 0, log[-600:]
    assert (lib / "One" / "Project" / "Alpha.xml").is_file() and not (lib / "Two" / "Project").exists(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))
    assert "Project/Alpha.xml" in ids(lib, "One") and "Project/Alpha.xml" not in ids(lib, "Two")


def sidebar_doc():
    code, lib, log = run("sidebar_doc", LOOSE_TO_TARGET + "wait 3000\nshot landed\n")
    assert code == 0, log[-600:]
    assert (lib / "Two" / "Target" / "Loose.xml").is_file() and not (lib / "One" / "Loose.xml").exists(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))


# 移动到 on Loose, Two opened in the dialog's tree (its arrow, at the row's left), Target in it, OK.
def opened(name): return f"downat move:{name} 0.045 0.5\nupat move:{name} 0.045 0.5\nwait 400\n"
MOVE = "rrow Loose\nwait 300\nrow Move to\nwait 500\n" + opened("Two") + "shot dialog\nanchor move:Target\nwait 300\nanchor move-ok\nwait 200\nshot ahead\n"


def move_dialog():
    code, lib, log = run("move_dialog", MOVE + "wait 3000\nshot landed\n")
    assert code == 0, log[-600:]
    assert (lib / "Two" / "Target" / "Loose.xml").is_file() and not (lib / "One" / "Loose.xml").exists(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))


def move_to_top():
    # Target, in Two (opened in the sidebar first): 移动到, One picked, OK.
    script = "hoverrow Two\nwait 300\nclick 59 195\nwait 600\nrrow Target\nwait 300\nrow Move to\nwait 500\nshot dialog\n" + opened("One") + "shot opened\nanchor move:One\nwait 300\nshot picked\nanchor move-ok\nwait 3000\nshot landed\n"
    code, lib, log = run("move_to_top", script)
    assert code == 0, log[-600:]
    assert (lib / "One" / "Target" / "Keep.xml").is_file() and not (lib / "Two" / "Target").exists(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))
    assert "Target/Keep.xml" in ids(lib, "One") and "Target/Keep.xml" not in ids(lib, "Two")


def move_lone():
    # One alone: Project opened in the sidebar, Alpha in it, 移动到 One's own row.
    script = "hoverrow Project\nwait 400\nclick 30 152\nwait 500\nrrow Alpha\nwait 300\nrow Move to\nwait 500\nshot dialog\nanchor move:One\nwait 300\nshot picked\nanchor move-ok\nwait 3000\nshot landed\n"
    code, lib, log = run("move_lone", script, tops=("One",))
    assert code == 0, log[-600:]
    assert (lib / "One" / "Alpha.xml").is_file() and not (lib / "One" / "Project" / "Alpha.xml").exists(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))


def open_page():
    # Alpha open (Project opened first, so it has a row); its folder dragged to Two; typed into after it landed.
    script = "hoverrow Project\nwait 400\nclick 30 152\nwait 500\nrow Alpha\nwait 600\n" + drag(152, (180, 50), (226, 400)) + "wait 3000\nshowing Alpha\nclick 450 187\nkey cmd-down\ntype  typed\nwait 2500\nshot saved\n"
    code, lib, log = run("open_page", script)
    assert code == 0, log[-600:]
    alpha = lib / "Two" / "Project" / "Alpha.xml"
    assert alpha.is_file() and "typed" in alpha.read_text(), sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml"))
    assert not (lib / "One" / "Project").exists(), "something was written to the old path"
    assert "Could not save" not in log


def fails():
    code, lib, log = run("fails", LOOSE_TO_TARGET + "wait 3000\nshot back\n", locked="Two/Target")
    assert code == 0, log[-600:]
    assert (lib / "One" / "Loose.xml").is_file() and not (lib / "Two" / "Target" / "Loose.xml").exists()
    assert "Target/Loose.xml" not in ids(lib, "Two")


CASES = {"sidebar_folder": sidebar_folder, "sidebar_doc": sidebar_doc, "move_dialog": move_dialog, "move_to_top": move_to_top, "move_lone": move_lone, "open_page": open_page, "fails": fails}
failed = []
for name in sys.argv[1:] or CASES:
    try:
        CASES[name]()
        print(f"ok    {name}")
    except Exception as e:
        failed.append(name)
        print(f"FAIL  {name}: {e}")
print(f"artifacts: {work}")
sys.exit(1 if failed else 0)
