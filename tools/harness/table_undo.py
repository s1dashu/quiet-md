"""⌘Z in a 数据表 is the table's, and 删除字段/列 from a column's menu, in a scratch library.

The report (2026-11-27): 创建副本 of a table file, the copy opened and a cell edited, ⌘Z undid the
创建副本 (「已撤销：创建副本」) instead of the cell. Here: the copy made from the sidebar's menu, opened,
a cell typed into; ⌘Z takes the cell back and leaves the copy; ⌘Z again (the table's history spent)
still leaves the copy; back on the sidebar (a click on a row's blank), ⌘Z is the folders' again and
undoes 创建副本. Then a column's menu: the title field's 删除字段/列 is there, dimmed; another
column's asks first (the records it holds data in) and goes; ⌘Z brings it back with its cells.

Build with --release --features devtools, then run this file. FDL_BIN selects another build;
TABLE_UNDO_SHOTS a folder to copy the screenshots to.
"""
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-table-undo-"))
source = work / "source"
source.mkdir(parents=True)
(source / ".quiet").mkdir()
(source / ".quiet" / "preferences.json").write_text(json.dumps({"appearance": "light", "english": False, "language": "zh-CN", "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False}))
db = sqlite3.connect(source / "Sources.sqlite")
db.executescript("""CREATE TABLE "Sources" ("名称" TEXT, "作者" TEXT); INSERT INTO "Sources" VALUES ('三体','刘慈欣'),('球状闪电','刘慈欣'),('雪崩','斯蒂芬森');""")
db.executescript("""CREATE TABLE _tables (id INTEGER PRIMARY KEY, name TEXT, position INTEGER, next_auto INTEGER);
CREATE TABLE _fields (id INTEGER PRIMARY KEY, table_id INTEGER, table_name TEXT, name TEXT, position INTEGER, type TEXT, options TEXT, description TEXT);
CREATE TABLE _views (id INTEGER PRIMARY KEY, table_id INTEGER, table_name TEXT, name TEXT, position INTEGER, type TEXT, config TEXT);
CREATE TABLE _meta (key TEXT PRIMARY KEY, value TEXT); INSERT INTO _meta VALUES ('format', '2'), ('app', 'Quiet.md');""")
db.commit()
db.close()

PROBE = os.environ.get("TABLE_UNDO_PROBE")
SHOTS = os.environ.get("TABLE_UNDO_SHOTS")


def run(name, commands, library):
    folder = work / name
    script = work / f"{name}.txt"
    script.write_text(f"resize 1300 850\nwait 1500\nmove 1200 800\n{commands}\nwait 600\nmove 1200 800\nshot end\n")
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(library), str(folder)], env=dict(os.environ, FDL_NOBLINK="1", FDL_ANIMATE="1"), check=True, timeout=180)
    if SHOTS:
        Path(SHOTS).mkdir(parents=True, exist_ok=True)
        for png in folder.glob("*.png"): shutil.copy(png, Path(SHOTS) / f"{name}-{png.name}")
    return folder / "lib"


def column(lib, file, name):
    """A column of the table in `file`, in row order; None when the table has no such column."""
    with sqlite3.connect(lib / file) as c:
        cols = [r[1] for r in c.execute('pragma table_info("Sources")')]
        if name not in cols: return None
        # a file the app never wrote to has no _order yet
        return [r[0] for r in c.execute(f'select "{name}" from "Sources" order by {"_order" if "_order" in cols else "rowid"}')]


def files(lib):
    return sorted(p.name for p in lib.glob("*.sqlite"))


if PROBE:
    run("probe", Path(PROBE).read_text(), source)
    print(work / "probe")
    sys.exit(0)

# where things are in a 1300×850 window (light theme, Chinese)
TITLE_1 = "460 149"             # 名称 of row 1
TITLE_HEAD, AUTHOR_HEAD = "400 112", "640 112"
ASK_DELETE = "811 464"          # the question's 删除

# the copy is written in the background: it opens once it is on disk
COPY = """rrow Sources
wait 500
row 创建副本
wait 4000
move 1200 800
shot copied
row Sources 副本
wait 1500
showing Sources 副本
"""
EDIT = f"""click {TITLE_1}
wait 300
type 改过
key enter
wait 800
move 1200 800
"""

# 1. the report: the copy's cell edited, ⌘Z takes the cell back, not the copy
lib = run("edit", COPY + EDIT + "shot edited\n", source)
assert files(lib) == ["Sources 副本.sqlite", "Sources.sqlite"], files(lib)
assert column(lib, "Sources 副本.sqlite", "名称") == ["改过", "球状闪电", "雪崩"], column(lib, "Sources 副本.sqlite", "名称")
assert column(lib, "Sources.sqlite", "名称") == ["三体", "球状闪电", "雪崩"]
lib = run("undo", COPY + EDIT + "key cmd-z\nwait 1200\nshot undone\n", source)
assert files(lib) == ["Sources 副本.sqlite", "Sources.sqlite"], files(lib)
assert column(lib, "Sources 副本.sqlite", "名称") == ["三体", "球状闪电", "雪崩"], column(lib, "Sources 副本.sqlite", "名称")
# 2. the table's history spent: ⌘Z again leaves the copy
lib = run("spent", COPY + EDIT + "key cmd-z\nwait 1200\nkey cmd-z\nwait 1500\nkey cmd-z\nwait 1500\nshot spent\n", source)
assert files(lib) == ["Sources 副本.sqlite", "Sources.sqlite"], files(lib)
assert column(lib, "Sources 副本.sqlite", "名称") == ["三体", "球状闪电", "雪崩"]
# ⌘⇧Z: the cell again
lib = run("redo", COPY + EDIT + "key cmd-z\nwait 1200\nkey cmd-shift-z\nwait 1200\n", source)
assert column(lib, "Sources 副本.sqlite", "名称") == ["改过", "球状闪电", "雪崩"], column(lib, "Sources 副本.sqlite", "名称")

# 3. 删除字段/列: the title field's is dimmed; another's asks, goes, and ⌘Z brings it back with its cells
DELETE = f"""row Sources
wait 1500
rclick {TITLE_HEAD}
wait 500
shot primary-menu
key escape
wait 300
rclick {AUTHOR_HEAD}
wait 500
shot field-menu
row 删除字段/列
wait 600
shot ask
click {ASK_DELETE}
wait 1000
move 1200 800
shot deleted
"""
lib = run("primary", f"row Sources\nwait 1500\nrclick {TITLE_HEAD}\nwait 500\nrow 删除字段/列\nwait 600\nkey escape\nwait 300\n", source)
assert column(lib, "Sources.sqlite", "名称") == ["三体", "球状闪电", "雪崩"]
lib = run("delete", DELETE, source)
assert column(lib, "Sources.sqlite", "作者") is None, column(lib, "Sources.sqlite", "作者")
lib = run("undelete", DELETE + "key cmd-z\nwait 1200\nshot undeleted\n", source)
assert column(lib, "Sources.sqlite", "作者") == ["刘慈欣", "刘慈欣", "斯蒂芬森"], column(lib, "Sources.sqlite", "作者")
lib = run("cancel", f"row Sources\nwait 1500\nrclick {AUTHOR_HEAD}\nwait 500\nrow 删除字段/列\nwait 600\nclick 754 464\nwait 600\n", source)
assert column(lib, "Sources.sqlite", "作者") == ["刘慈欣", "刘慈欣", "斯蒂芬森"]

print("table_undo: all passed", work)
