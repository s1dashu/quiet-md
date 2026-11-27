"""置顶's own order and the top-level folders' order, dragged by hand (crates/core/src/pins.rs, src/pin_drag.rs).

Build with --release --features devtools, then run this file. Two top-level folders, One
(Alpha, Beta) and Two (Gamma, Delta), listed in their own folders.json (`QUIET_MD_FOLDERS`):

  pin_new     Alpha, Gamma, Beta pinned in turn: each new pin on top (Beta, Gamma, Alpha), across both
  pin_drag    then Alpha dragged above Beta: Alpha, Beta, Gamma, on screen and in the libraries' files
  pin_undo    the same, then ⌘Z: Beta, Gamma, Alpha again, in one step
  top_drag    Two's heading dragged above One's: folders.json lists Two first
  top_undo    the same, then ⌘Z: One first again
  legacy      a 0.1.14 library (FORMAT 4, one top-level folder): its pins first, last changed first,
              then its 收藏, latest first, as it opens and is upgraded

Everything happens in a temporary library; nothing else is touched. Pass case names to run
only those. Screenshots are left in the printed folder.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-pins-order-"))
PREFS = json.dumps({"appearance": "light", "english": True, "language": "en", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False})


def two_folders(src):
    for name, pages in (("One", ["Alpha", "Beta"]), ("Two", ["Gamma", "Delta"])):
        (src / name / ".quiet").mkdir(parents=True)
        (src / name / ".quiet" / "preferences.json").write_text(PREFS)
        for t in pages: (src / name / f"{t}.xml").write_text(f"<title>{t}</title>\n<p>{t} text</p>\n")
    return src


def run(name, script, src, tops=True):
    out = work / name
    out.mkdir(parents=True)
    lib = out / "lib"
    shutil.copytree(src, lib)
    folders = out / "folders.json"
    if tops: folders.write_text(json.dumps({"v": 1, "folders": [{"key": "k-one", "path": str(lib / "One"), "added": 1}, {"key": "k-two", "path": str(lib / "Two"), "added": 2}]}))
    (out / "s.txt").write_text("resize 1300 850\nwait 300\nmove 700 500\nwait 1200\n" + script + "\nquit\n")
    binary = os.environ.get("FDL_BIN") or str(here.parent.parent / "target" / "release" / "quiet_md")
    env = {"HOME": os.path.expanduser("~"), "QUIET_MD_HOME": str(lib / "One" if tops else lib), "FDL_SCRIPT": str(out / "s.txt"), "FDL_SHOTS": str(out),
           "PATH": "/usr/bin:/bin", "FDL_NOBLINK": "1", "FDL_ANIMATE": "1"}
    if tops: env["QUIET_MD_FOLDERS"] = str(folders)
    if "TMPDIR" in os.environ: env["TMPDIR"] = os.environ["TMPDIR"]
    code = subprocess.run(binary, env=env, cwd=out, stdout=open(out / "log", "w"), stderr=subprocess.STDOUT, timeout=180).returncode
    log = (out / "log").read_text(errors="replace")
    assert code == 0, log[-800:]
    return lib, folders, log


def pins_on_disk(*tops):
    """The pinned pages of top-level folders `tops` by title, in 置顶's order (their `pin_order` keys)."""
    out = []
    for top in tops:
        fields = {}
        for f in (top / ".quiet" / "meta").rglob("*.json"):
            if f.name == "device.json": continue
            for id, item in json.loads(f.read_text()).get("items", {}).items():
                have = fields.setdefault(id, {})
                for k, v in item.items():
                    if isinstance(v, list) and len(v) == 2 and (k not in have or v[1] > have[k][1]): have[k] = v
        for f in fields.values():
            if (f.get("pinned") or [False])[0] and (f.get("path") or [None])[0]:
                out.append(((f.get("pin_order") or ["~"])[0], Path(f["path"][0]).stem))
    return [t for _, t in sorted(out)]


def on_screen(log, names, nth=-1):
    """`names` in the order the last (`nth`) `moveat` of each found them, top to bottom (the topmost
    row of a title is its 置顶 row)."""
    at = {n: float(m[nth]) for n in names if (m := re.findall(rf'devtools: moveat "{n}" at [\d.]+ ([\d.]+)', log))}
    return sorted(at, key=at.get)


def look(names): return "".join(f"moveat {n}\nwait 50\n" for n in names) + "move 700 500\nwait 300\n"


PIN = "row Two\nwait 800\n" + "".join(f"rrow {t}\nwait 400\nrow Pin\nwait 600\n" for t in ["Alpha", "Gamma", "Beta"])


def drag(src, to, fy, shot):
    """Pick row `src` up at its middle, carry it to `fy` of row `to`'s height and let go there. `to`
    is a row that stays put while the others make room (置顶's heading), or the drop chases it."""
    return (f"downat {src}\nwait 50\ndragby 0 6\nwait 50\ndragby 0 6\nwait 100\ndragat {to} 0.5 {fy}\nwait 120\ndragat {to} 0.5 {fy}\nwait 700\nshot {shot}\n"
            + f"upat {to} 0.5 {fy}\nwait 1200\n")


PINS = ["Alpha", "Beta", "Gamma"]


def pin_new():
    lib, _, log = run("pin_new", PIN + "shot pins-new\n" + look(PINS), two_folders(work / "src-pin_new"))
    assert on_screen(log, PINS) == ["Beta", "Gamma", "Alpha"], on_screen(log, PINS)
    assert pins_on_disk(lib / "One", lib / "Two") == ["Beta", "Gamma", "Alpha"], pins_on_disk(lib / "One", lib / "Two")


def pin_drag():
    lib, _, log = run("pin_drag", PIN + drag("Alpha", "Pinned", 1.4, "pins-gap") + "shot pins-after\n" + look(PINS), two_folders(work / "src-pin_drag"))
    assert on_screen(log, PINS) == ["Alpha", "Beta", "Gamma"], on_screen(log, PINS)
    assert pins_on_disk(lib / "One", lib / "Two") == ["Alpha", "Beta", "Gamma"], pins_on_disk(lib / "One", lib / "Two")
    # a click that does not move still opens the page
    assert "devtools: downat \"Alpha\"" in log


def pin_undo():
    lib, _, log = run("pin_undo", PIN + drag("Alpha", "Pinned", 1.4, "pins-gap") + look(PINS) + "key cmd-z\nwait 1000\nshot pins-undone\n" + look(PINS), two_folders(work / "src-pin_undo"))
    assert on_screen(log, PINS, 0) == ["Alpha", "Beta", "Gamma"], on_screen(log, PINS, 0)
    assert on_screen(log, PINS) == ["Beta", "Gamma", "Alpha"], on_screen(log, PINS)
    assert pins_on_disk(lib / "One", lib / "Two") == ["Beta", "Gamma", "Alpha"], pins_on_disk(lib / "One", lib / "Two")


def tops_listed(folders): return [Path(f["path"]).name for f in json.loads(folders.read_text())["folders"]]


def top_drag():
    _, folders, log = run("top_drag", "shot tops-before\n" + drag("Two", "One", 0.1, "tops-gap") + "shot tops-after\n" + look(["One", "Two"]), two_folders(work / "src-top_drag"))
    assert tops_listed(folders) == ["Two", "One"], tops_listed(folders)
    assert on_screen(log, ["One", "Two"]) == ["Two", "One"], on_screen(log, ["One", "Two"])


def top_undo():
    _, folders, log = run("top_undo", drag("Two", "One", 0.1, "tops-gap") + look(["One", "Two"]) + "key cmd-z\nwait 1000\nshot tops-undone\n" + look(["One", "Two"]), two_folders(work / "src-top_undo"))
    assert on_screen(log, ["One", "Two"], 0) == ["Two", "One"], on_screen(log, ["One", "Two"], 0)
    assert tops_listed(folders) == ["One", "Two"], tops_listed(folders)
    assert on_screen(log, ["One", "Two"]) == ["One", "Two"], on_screen(log, ["One", "Two"])


def legacy():
    """0.1.14's `.quiet/index.json` (FORMAT 4): pinned P1 (changed earlier) and P2, 收藏 S1 and S2 (starred later)."""
    src = work / "src-legacy"
    (src / ".quiet").mkdir(parents=True)
    (src / ".quiet" / "preferences.json").write_text(PREFS)
    (src / ".quiet" / "format.json").write_text(json.dumps({"format": 4}))
    index = {}
    for i, (t, pinned, starred, modified) in enumerate([("P1", True, 0, 1_700_000_100), ("P2", True, 0, 1_700_000_200), ("S1", False, 1_690_000_000_000, 1_700_000_300), ("S2", False, 1_695_000_000_000, 1_700_000_050), ("Plain", False, 0, 1_700_000_400)]):
        (src / f"{t}.xml").write_text(f"<title>{t}</title>\n<p>{t}</p>\n")
        e = {"id": f"00000000-0000-4000-8000-00000000000{i}", "created": 1_690_000_000, "modified": modified}
        if pinned: e["pinned"] = True
        if starred: e["starred"] = starred
        index[f"{t}.xml"] = e
    (src / ".quiet" / "index.json").write_text(json.dumps(index))
    names = ["P1", "P2", "S1", "S2"]
    lib, _, log = run("legacy", "shot legacy\n" + look(names), src, tops=False)
    assert on_screen(log, names) == ["P2", "P1", "S2", "S1"], on_screen(log, names)
    assert (lib / ".quiet" / "index.json.v4").is_file()


CASES = {"pin_new": pin_new, "pin_drag": pin_drag, "pin_undo": pin_undo, "top_drag": top_drag, "top_undo": top_undo, "legacy": legacy}

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
