"""手动排序 by dragging: a folder page's grid and list, and the sidebar.

Build with --release --features devtools, then run this file. Each case runs in
a temporary library (Project/{Alpha, Beta, Gamma, Delta, Box/Inner}) under
手动排序 and reads the order back from the library's own files
(`.quiet/meta`, the hand order's keys):

  grid        a document card dropped on the gap after another card takes that place
  grid_undo   the same, then ⌘Z: the order as it was, in one step
  grid_into   a document card dropped on a folder card's middle goes inside it
  grid_folder a folder card dropped on the gap before the first card
  list        the list: a row dropped on the upper half of another goes before it
  sidebar     the sidebar: a row dropped between two others
  top         the top-level folder's own page (Project and Loose there): a card put first
  auto        under 标题 (another sort) a drop beside a card changes nothing; into a folder still works

Pass case names to run only those. Screenshots are left in the printed folder.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-manual-order-"))
PREFS = json.dumps({"appearance": "light", "english": True, "language": "en", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False})


def library(root):
    (root / ".quiet").mkdir(parents=True)
    (root / ".quiet" / "preferences.json").write_text(PREFS)
    (root / "Project" / "Box").mkdir(parents=True)
    for t in ["Alpha", "Beta", "Gamma", "Delta"]:
        (root / "Project" / f"{t}.xml").write_text(f"<title>{t}</title>\n<p>{t} text</p>\n")
    (root / "Project" / "Box" / "Inner.xml").write_text("<title>Inner</title>\n<p>inner</p>\n")
    (root / "Loose.xml").write_text("<title>Loose</title>\n<p>loose</p>\n")
    return root


def run(name, script):
    out = work / name
    (work / f"{name}.txt").write_text("resize 1300 850\nwait 300\nmove 700 500\nwait 1200\n" + script)
    r = subprocess.run([sys.executable, str(here / "run.py"), str(work / f"{name}.txt"), str(library(work / f"src-{name}")), str(out)],
                       env=dict(os.environ, FDL_NOBLINK="1", FDL_ANIMATE="1"), timeout=180)
    log = (out / "log").read_text(errors="replace") if (out / "log").exists() else ""
    assert r.returncode == 0, log[-800:]
    return out / "lib", log


def order(lib, folder="Project"):
    """`folder`'s pages and folders by name, in their hand order (the newest write of each field wins)."""
    fields = {}
    for f in (lib / ".quiet" / "meta").rglob("*.json"):
        if f.name == "device.json": continue
        for id, item in json.loads(f.read_text()).get("items", {}).items():
            have = fields.setdefault(id, {})
            for k, v in item.items():
                if isinstance(v, list) and len(v) == 2 and (k not in have or v[1] > have[k][1]): have[k] = v
    kids = []
    for id, f in fields.items():
        path = f.get("path", [None])[0]
        if not path or "gone" in f or "trashed" in f: continue
        p = path.rstrip("/")
        if (p.rsplit("/", 1)[0] if "/" in p else "") != folder: continue
        # At the top of the top-level folder the place is `root_order`.
        kids.append((f.get("order" if folder else "root_order", ["~"])[0], id, Path(p).stem))
    return [n for _, _, n in sorted(kids)]


# Into Project's page, as a grid or a list.
OPEN = "row Project\nwait 800\n"
LIST = "anchor page-view\nwait 300\nrow List\nwait 500\n"
TITLE = "anchor page-view\nwait 300\nrow Title\nwait 500\n"


def drag(src, to, fx, fy, shot=None):
    """Pick `src` up at its middle, carry it to `fx fy` of `to` and let go there."""
    s = f"downat {src}\nwait 50\ndragby 6 6\nwait 50\ndragby 6 6\nwait 100\ndragat {to} {fx} {fy}\nwait 120\ndragat {to} {fx} {fy}\nwait 700\n"
    if shot: s += f"shot {shot}\n"
    return s + f"upat {to} {fx} {fy}\nwait 1500\n"


def first(name):
    """The order a fresh library starts with (手动排序 freezes it on first open)."""
    lib, _ = run(f"{name}-start", OPEN)
    return order(lib)


def moved(start, item, before):
    """`start` with `item` put before `before` (None: last)."""
    v = [x for x in start if x != item]
    v.insert(v.index(before) if before else len(v), item)
    return v


def grid():
    start = first("grid")
    a, b = start[0], start[1]
    last = start[-1]
    lib, _ = run("grid", OPEN + "shot before\n" + drag(f"card:{last}", f"card:{a}", 0.85, 0.5, "gap") + "shot after\n")
    assert order(lib) == moved(start, last, b), (start, order(lib))
    return start


def grid_undo():
    start = first("grid_undo")
    a, last = start[0], start[-1]
    lib, _ = run("grid_undo", OPEN + drag(f"card:{last}", f"card:{a}", 0.85, 0.5) + "key cmd-z\nwait 1500\nshot undone\n")
    assert order(lib) == start, (start, order(lib))


def grid_into():
    start = first("grid_into")
    doc = next(x for x in start if x != "Box")
    lib, _ = run("grid_into", OPEN + drag(f"card:{doc}", "card:Box", 0.5, 0.6, "over-folder") + "shot inside\n")
    assert (lib / "Project" / "Box" / f"{doc}.xml").is_file(), sorted(p.name for p in (lib / "Project").rglob("*"))
    assert order(lib) == [x for x in start if x != doc], order(lib)


def grid_folder():
    start = first("grid_folder")
    head = start[0] if start[0] != "Box" else start[1]
    lib, _ = run("grid_folder", OPEN + drag("card:Box", f"card:{head}", 0.12, 0.5, "folder-gap") + "shot folder-first\n")
    assert order(lib)[0] == "Box", order(lib)
    assert (lib / "Project" / "Box" / "Inner.xml").is_file()


def list_view():
    start = first("list")
    a, last = start[1], start[-1]
    lib, _ = run("list", OPEN + LIST + "shot list-before\n" + drag(f"list:{last}", f"list:{a}", 0.5, 0.2, "list-gap") + "shot list-after\n")
    assert order(lib) == moved(start, last, a), (start, order(lib))


def sidebar():
    start = first("sidebar")
    a, last = start[1], start[-1]
    lib, _ = run("sidebar", OPEN + drag(last, a, 0.5, 0.15, "sidebar-gap") + "shot sidebar-after\n")
    assert order(lib) == moved(start, last, a), (start, order(lib))


def top():
    """The top-level folder's own page: its pages and folders keep their place in `root_order`."""
    lib, _ = run("top-start", "row lib\nwait 800\n")
    start = order(lib, "")
    a, last = start[0], start[-1]
    lib, _ = run("top", "row lib\nwait 800\nshot top-before\n" + drag(f"card:{last}", f"card:{a}", 0.15, 0.5, "top-gap") + "shot top-after\n")
    assert order(lib, "") == moved(start, last, a), (start, order(lib, ""))


def auto():
    start = first("auto")
    a, last = start[0], start[-1]
    doc = next(x for x in start if x not in ("Box", a))
    lib, _ = run("auto", OPEN + TITLE + "shot auto-before\n" + drag(f"card:{last}", f"card:{a}", 0.85, 0.5, "auto-gap") + "shot auto-after\n"
                 + drag(f"card:{doc}", "card:Box", 0.5, 0.6) + "shot auto-into\n")
    assert order(lib) == [x for x in start if x != doc], (start, order(lib))
    assert (lib / "Project" / "Box" / f"{doc}.xml").is_file()


CASES = {"grid": grid, "grid_undo": grid_undo, "grid_into": grid_into, "grid_folder": grid_folder, "list": list_view, "sidebar": sidebar, "top": top, "auto": auto}

if __name__ == "__main__":
    failed = []
    for name in sys.argv[1:] or CASES:
        try:
            CASES[name]()
            print(f"ok   {name}")
        except AssertionError as e:
            failed.append(name)
            print(f"FAIL {name}: {e}")
    print(f"shots and logs: {work}")
    sys.exit(1 if failed else 0)
