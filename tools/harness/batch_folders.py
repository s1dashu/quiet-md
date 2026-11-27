"""批量管理 with folders (docs/design/BATCH_FOLDERS.md): folders check like documents
in a folder page's grid and list and in the sidebar, and 创建副本, 移动到 and 删除
take them along with everything in them, each undone in one step.

Build with --release --features devtools, then run this file. Each case runs in
a temporary library and checks the files on disk:

  grid      the grid: a folder card and a document card checked (the folder not
            dimmed, its box where a card's is); 创建副本 copies the folder with its
            subfolders and pages beside it, in the background; ⌘Z takes the copies away
  list      the list: a folder and a document checked; 移动到 another folder takes
            the folder's pages along, its links following; ⌘Z puts them back
  sidebar   the sidebar: a folder and a page inside it checked (Shift-click); 删除
            asks with 「包括其中的 2 篇文档」 and trashes the folder once, as a whole;
            ⌘Z brings it all back
  sidebar_copy  the same pick duplicated: the page inside is copied once, with its
            folder (上层优先), not again beside itself

Everything happens in a temporary library; no system clipboard or user
library is accessed. Pass case names to run only those. Screenshots of each
case are left in the artifacts folder.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-batch-folders-"))
PREFS = json.dumps({"appearance": "light", "english": True, "language": "en", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False})


def library(root):
    """Project/{Alpha, Beta, Sub/{Inner, Deep/Leaf}, Other/Keep}, and Outside, which Inner links to."""
    (root / ".quiet").mkdir(parents=True)
    (root / ".quiet" / "preferences.json").write_text(PREFS)
    (root / "Project" / "Sub" / "Deep").mkdir(parents=True)
    (root / "Project" / "Other").mkdir(parents=True)
    (root / "Project" / "Alpha.xml").write_text("<title>Alpha</title>\n<p>alpha</p>\n")
    (root / "Project" / "Beta.xml").write_text("<title>Beta</title>\n<p>beta</p>\n")
    (root / "Project" / "Sub" / "Inner.xml").write_text('<title>Inner</title>\n<p>To <cite type="doc" href="../../Outside.xml" file-type="docx">Outside</cite></p>\n')
    (root / "Project" / "Sub" / "Deep" / "Leaf.xml").write_text("<title>Leaf</title>\n<p>leaf</p>\n")
    (root / "Project" / "Other" / "Keep.xml").write_text("<title>Keep</title>\n<p>keep</p>\n")
    (root / "Outside.xml").write_text("<title>Outside</title>\n<p>outside</p>\n")
    return root


def run(name, script):
    out = work / name
    (work / f"{name}.txt").write_text("resize 1300 850\nwait 300\nmove 700 500\nwait 1200\n" + script)
    r = subprocess.run([sys.executable, str(here / "run.py"), str(work / f"{name}.txt"), str(library(work / f"src-{name}")), str(out)],
                       env=dict(os.environ, FDL_NOBLINK="1"), timeout=180)
    log = (out / "log").read_text(errors="replace") if (out / "log").exists() else ""
    assert r.returncode == 0, log[-800:]
    return out / "lib", log


def names(d):
    return sorted(p.name for p in d.iterdir() if not p.name.startswith("."))


# Into a folder page, shown as `view`, and into 批量管理.
OPEN = "row Project\nwait 800\nanchor page-view\nwait 300\nrow {view}\nwait 500\nanchor tb-more\nwait 300\nrow Manage\nwait 400\n"


def grid():
    script = OPEN.format(view="Grid") + "anchor card:Sub\nwait 200\nanchor card:Alpha\nwait 400\nshot checked\nanchor page-batch-copy\nwait 2500\nshot copied\n"
    lib, log = run("grid", script)
    project = lib / "Project"
    assert (project / "Sub copy" / "Inner.xml").is_file() and (project / "Sub copy" / "Deep" / "Leaf.xml").is_file(), names(project)
    assert (project / "Alpha copy.xml").is_file(), names(project)
    assert 'href="../../Outside.xml"' in (project / "Sub copy" / "Inner.xml").read_text(), "the copy's link no longer reaches Outside"
    assert (project / "Sub" / "Deep" / "Leaf.xml").is_file(), "the original moved"
    # ⌘Z: one step takes every copy away.
    lib, log = run("grid-undo", script + "key cmd-z\nwait 2500\nshot undone\n")
    project = lib / "Project"
    assert not (project / "Sub copy").exists() and not (project / "Alpha copy.xml").exists(), names(project)
    assert (project / "Sub" / "Deep" / "Leaf.xml").is_file() and (project / "Alpha.xml").is_file()


def list_():
    script = OPEN.format(view="List") + "anchor list:Sub\nwait 200\nanchor list:Beta\nwait 400\nshot checked\nanchor page-batch-move\nwait 600\ntype Other\nwait 400\nanchor move:Other\nwait 300\nshot dialog\nanchor move-ok\nwait 2500\nshot moved\n"
    lib, log = run("list", script)
    project = lib / "Project"
    assert (project / "Other" / "Sub" / "Deep" / "Leaf.xml").is_file() and (project / "Other" / "Beta.xml").is_file(), names(project / "Other")
    assert not (project / "Sub").exists() and not (project / "Beta.xml").exists(), names(project)
    assert 'href="../../../Outside.xml"' in (project / "Other" / "Sub" / "Inner.xml").read_text(), "the moved page's link did not follow"
    lib, log = run("list-undo", script + "key cmd-z\nwait 2500\nshot undone\n")
    project = lib / "Project"
    assert (project / "Sub" / "Deep" / "Leaf.xml").is_file() and (project / "Beta.xml").is_file(), names(project)
    assert not (project / "Other" / "Sub").exists(), names(project / "Other")
    assert 'href="../../Outside.xml"' in (project / "Sub" / "Inner.xml").read_text()


# Sub opened (so its pages show in the sidebar), then Sub and Inner, a page inside it, Shift-clicked into 批量管理.
PICK = "row Project\nwait 600\nrow Sub\nwait 600\nmods shift\nrow Sub\nwait 200\nrow Inner\nwait 200\nmods none\nrow Alpha\nwait 400\nshot checked\n"


def sidebar():
    script = PICK + "rrow Alpha\nwait 300\nrow Delete\nwait 400\nshot confirm\nkey enter\nwait 2500\nshot deleted\n"
    lib, log = run("sidebar", script)
    project = lib / "Project"
    assert not (project / "Sub").exists() and not (project / "Alpha.xml").exists(), names(project)
    trash = lib / ".quiet" / "trash"
    # The 回收站 keeps entries by id: Alpha, Inner and Leaf, Sub and Deep, each once.
    kept = sorted(p.suffix or "dir" for p in trash.iterdir())
    assert kept == [".xml", ".xml", ".xml", "dir", "dir"], f"the folder did not go to the Trash with what was in it: {kept}"
    lib, log = run("sidebar-undo", script + "row Project\nwait 400\nkey cmd-z\nwait 2500\nshot undone\n")
    project = lib / "Project"
    assert (project / "Sub" / "Inner.xml").is_file() and (project / "Sub" / "Deep" / "Leaf.xml").is_file() and (project / "Alpha.xml").is_file(), names(project)


def sidebar_copy():
    lib, log = run("sidebar_copy", PICK + "rrow Alpha\nwait 300\nrow Duplicate\nwait 2500\nshot copied\n")
    project = lib / "Project"
    assert (project / "Sub copy" / "Inner.xml").is_file() and (project / "Alpha copy.xml").is_file(), names(project)
    assert not (project / "Sub" / "Inner copy.xml").exists(), "a page inside a picked folder was copied twice"


CASES = {"grid": grid, "list": list_, "sidebar": sidebar, "sidebar_copy": sidebar_copy}

if __name__ == "__main__":
    for name in sys.argv[1:] or list(CASES):
        CASES[name]()
        print(f"ok {name}")
    print(f"screenshots: {work}")
