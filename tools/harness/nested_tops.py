"""Two top-level folders, one inside the other (docs/design/workspaces.md 2.6).

Build with --release --features devtools, then run this file. Outer holds Inner, and
both are top-level folders (`QUIET_MD_FOLDERS`); Inner's pages show under Outer's
folder Inner and under Inner's own heading. Each case changes them through one side
and checks, after the 2 s poll, that the other side shows the same rows, once each:

  inner_side  through Inner: a page renamed, one deleted (into Inner's 回收站 only), a
              new one made and titled; Outer's folder Inner then lists them too
  outer_side  the same through Outer's folder Inner; Inner's heading then lists them
  search      the search lists a page of Inner once, not once per top-level folder
  system      添加文件夹 on /usr: refused with a toast, nothing added
  two_windows Note open through Inner in a window of its own and through Outer in the
              main one, typed into by turns: each shows the other's typing, and the
              file ends with all of it (two_windows_md: the same for a Markdown page)
  pin_rename  Note pinned through Inner, renamed through Outer: still pinned in Inner
  rename_top  Outer's folder Inner renamed: Inner's heading follows to the new name
              (folders.json too), its pin kept
  delete_top  Outer's folder Inner deleted: the confirmation says it is a top-level
              folder too; its heading goes, the folder into Outer's 回收站
              (delete_undo: then ⌘Z brings back both)

What each side shows is read from the `row` step's list of the rows it could see (the
last step looks for a row that is not there, so the run ends with exit code 3).
Everything happens in a temporary folder with a temporary HOME; no system clipboard or
user library is accessed. Pass case names to run only those. Screenshots of each case
are left in the artifacts folder.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-nested-tops-"))
PREFS = json.dumps({"appearance": "light", "english": True, "language": "en", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False})
PROBE = "hoverrow __rows__"


def run(name, script, note=("Note.xml", "<title>Note</title>\n<p>anchor</p>\n")):
    """The script on a fresh Outer/Inner; the rows it saw last, and the library."""
    out = work / name
    lib = out / "lib"
    for d in ("Outer", "Outer/Inner"):
        (lib / d / ".quiet").mkdir(parents=True)
        (lib / d / ".quiet" / "preferences.json").write_text(PREFS)
    (lib / "Outer/Inner" / note[0]).write_text(note[1])
    (lib / "Outer/Inner/Second.xml").write_text("<title>Second</title>\n<p>s</p>\n")
    (lib / "Outer/Top.xml").write_text("<title>Top</title>\n<p>t</p>\n")
    folders = out / "folders.json"
    folders.write_text(json.dumps({"v": 1, "folders": [{"key": "k-outer", "path": str(lib / "Outer"), "added": 1}, {"key": "k-inner", "path": str(lib / "Outer/Inner"), "added": 2}]}))
    (out / "home").mkdir()
    (out / "s.txt").write_text("resize 1300 850\nwait 300\nmove 700 500\nwait 1200\n" + script + "\n" + PROBE + "\n")
    binary = os.environ.get("FDL_BIN") or str(here.parent.parent / "target" / "release" / "quiet_md")
    env = {"HOME": str(out / "home"), "QUIET_MD_HOME": str(lib / "Outer"), "QUIET_MD_FOLDERS": str(folders), "FDL_SCRIPT": str(out / "s.txt"), "FDL_SHOTS": str(out),
           "PATH": "/usr/bin:/bin", "FDL_NOBLINK": "1", "FDL_ANIMATE": "1"}
    if "TMPDIR" in os.environ: env["TMPDIR"] = os.environ["TMPDIR"]
    code = subprocess.run(binary, env=env, cwd=out, stdout=open(out / "log", "w"), stderr=subprocess.STDOUT, timeout=180).returncode
    log = (out / "log").read_text(errors="replace")
    seen = re.search(r'titled "__rows__"; shown: (\[.*\])', log)
    assert code == 3 and seen, f"exit {code}: {log[-800:]}"
    run.folders = json.loads(folders.read_text())
    return json.loads(seen.group(1)), lib


def pages(lib):
    return sorted(str(p.relative_to(lib)) for p in lib.rglob("*.xml") if ".quiet" not in p.parts)


def trash(lib, d):
    t = lib / d / ".quiet" / "trash"
    return len(list(t.iterdir())) if t.is_dir() else 0


# Every run starts as: Outer's heading (y 59) open with Top (90) and its folder Inner (121) shut,
# then Inner's heading (164) shut. Opening Outer's folder Inner (its icon, x 30) puts Inner's
# heading at 226. A row's menu: Rename / Delete; Delete asks first (its button at 794 473).
OPEN_INNER = "click 59 164\nwait 600\n"
OPEN_FOLDER = "hoverrow Inner\nwait 400\nclick 30 121\nwait 600\n"
RENAME = "rrow Note\nwait 300\nrow Rename\nwait 300\nkey cmd-a\ntype Renamed\nkey enter\nwait 600\n"
DELETE = "rrow Second\nwait 300\nrow Delete\nwait 400\nclick 794 473\nwait 800\n"
# Both sides as they end, after the other has had its 2 s poll and the new page's file its name.
BOTH = ["Outer", "Top", "Inner", "Fresh", "Renamed", "Inner", "Fresh", "Renamed", "Settings"]


def inner_side():
    # The new page: ⌘N with Renamed (Inner's) in front goes to Inner.
    script = OPEN_INNER + "shot start\n" + RENAME + DELETE + "row Renamed\nwait 800\nshowing Renamed\nkey cmd-n\nwait 800\ntype Fresh\nwait 3500\nshot inner_done\n" + OPEN_FOLDER + "wait 6000\nshot both\n"
    rows, lib = run("inner_side", script)
    assert rows == BOTH, rows
    assert pages(lib) == ["Outer/Inner/Fresh.xml", "Outer/Inner/Renamed.xml", "Outer/Top.xml"], pages(lib)
    assert (trash(lib, "Outer/Inner"), trash(lib, "Outer")) == (1, 0), "deleted once, into the 回收站 of the side it was deleted through"


def outer_side():
    # The new page: Outer's folder Inner's "+" › Document.
    script = OPEN_FOLDER + "shot start\n" + RENAME + DELETE + "hoverrow Inner\nwait 300\nclick 250 121\nwait 500\nrow Document\nwait 800\ntype Fresh\nwait 3500\nshot outer_done\nclick 59 226\nwait 6000\nshot both\n"
    rows, lib = run("outer_side", script)
    assert rows == ["Outer", "Top", "Inner", "Renamed", "Fresh", "Inner", "Fresh", "Renamed", "Settings"], rows
    assert pages(lib) == ["Outer/Inner/Fresh.xml", "Outer/Inner/Renamed.xml", "Outer/Top.xml"], pages(lib)
    assert (trash(lib, "Outer"), trash(lib, "Outer/Inner")) == (1, 0)


def search():
    # Both sides open, so each lists Note; the search shows it once (the screenshot), Top too.
    rows, lib = run("search", OPEN_INNER + OPEN_FOLDER + "key cmd-j\nwait 400\ntype o\nwait 600\nshot search\nkey escape\nwait 300\n")
    assert rows.count("Note") == 2 and rows.count("Second") == 2, rows


def system():
    rows, lib = run("system", "pick /usr\nkey cmd-shift-o\nwait 700\nshot system\n")
    assert [f["path"] for f in run.folders["folders"]] == [str(lib / "Outer"), str(lib / "Outer/Inner")], run.folders
    assert "Inner" in rows and "usr" not in rows, rows


# The caret goes after "anchor" (find, then →) and the text goes in there: each window in turn.
def typing(win, word):
    return f"window {win}\nkey cmd-f\nwait 200\ntype anchor\nwait 300\nkey escape\nwait 200\nkey right\ntype  {word}\nwait 400\n"


def two_windows(note=None, name="two_windows"):
    words = ["Alpha1", "Beta1", "Alpha2", "Beta2", "Alpha3", "Beta3"]
    # Window 1: Inner's Note (only Inner's heading open). Window 0: Outer's Note, the topmost once Outer's folder Inner is open.
    script = OPEN_INNER + "rrow Note\nwait 300\nrow Open in new window\nwait 1500\nwindows 2\nwindow 0\nrow Inner\nwait 600\nrow Note\nwait 800\nshowing Note\n"
    for i, w in enumerate(words): script += typing(i % 2, w) + ("wait 2500\n" if i in (1, 4) else "")
    script += "wait 3000\nwindow 1\nshot window1\nwindow 0\nshot window0\nwait 2500\n"
    rows, lib = run(name, script, *([note] if note else []))
    text = (lib / "Outer/Inner" / (note[0] if note else "Note.xml")).read_text()
    lost = [w for w in words if w not in text]
    assert not lost, f"lost {lost}: {text}"


def two_windows_md():
    two_windows(("Note.md", "# Note\n\nanchor\n"), "two_windows_md")


def pin_rename():
    # Pinned through Inner (its row tops the 置顶 list); Outer's own Note is then at 77 227.
    script = OPEN_INNER + "rrow Note\nwait 300\nrow Pin\nwait 600\nrow Inner\nwait 600\nrclick 77 227\nwait 300\nrow Rename\nwait 300\nkey cmd-a\ntype Renamed\nkey enter\nwait 6000\nshot pin_rename\n"
    rows, lib = run("pin_rename", script)
    assert rows[:2] == ["Pinned", "Renamed"] and "Note" not in rows, rows
    assert pages(lib) == ["Outer/Inner/Renamed.xml", "Outer/Inner/Second.xml", "Outer/Top.xml"], pages(lib)


def rename_top():
    # Inner's Note pinned; then Outer's folder Inner (the topmost Inner row) renamed, looked at before the 2 s look.
    script = OPEN_INNER + "rrow Note\nwait 300\nrow Pin\nwait 600\nrrow Inner\nwait 300\nrow Rename\nwait 300\nkey cmd-a\ntype Moved\nkey enter\nwait 1200\nshot rename_top\n"
    rows, lib = run("rename_top", script)
    assert rows.count("Moved") == 2 and "Inner" not in rows and rows[:2] == ["Pinned", "Note"], rows
    assert [f["path"] for f in run.folders["folders"]] == [str(lib / "Outer"), str(lib / "Outer/Moved")], run.folders
    assert not (lib / "Outer/Inner").exists() and (lib / "Outer/Moved/.quiet").is_dir()


DELETE_TOP = "rrow Inner\nwait 300\nrow Delete\nwait 500\nshot confirm\nclick 794 493\nwait 2500\nshot deleted\n"


def delete_top():
    rows, lib = run("delete_top", DELETE_TOP)
    assert "Inner" not in rows and "Note" not in rows, rows
    assert [f["path"] for f in run.folders["folders"]] == [str(lib / "Outer")], run.folders
    assert not (lib / "Outer/Inner").exists() and any((d / ".quiet/format.json").is_file() for d in (lib / "Outer/.quiet/trash").iterdir()), "into Outer's 回收站 with its .quiet"


def delete_undo():
    rows, lib = run("delete_undo", DELETE_TOP + "key cmd-z\nwait 3000\nshot undone\n")
    assert rows.count("Inner") == 2, rows
    assert [f["path"] for f in run.folders["folders"]] == [str(lib / "Outer"), str(lib / "Outer/Inner")], run.folders
    assert (lib / "Outer/Inner/Note.xml").exists() and (lib / "Outer/Inner/.quiet").is_dir()


CASES = {"inner_side": inner_side, "outer_side": outer_side, "search": search, "system": system, "two_windows": two_windows, "two_windows_md": two_windows_md,
         "pin_rename": pin_rename, "rename_top": rename_top, "delete_top": delete_top, "delete_undo": delete_undo}
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
