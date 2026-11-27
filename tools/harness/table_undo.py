"""⌘Z in a 数据表 is the table's, and 删除字段/列 from a column's menu, in a scratch library.

The report (2026-11-27): 创建副本 of a table file, the copy opened and a cell edited, ⌘Z undid the
创建副本 (「已撤销：创建副本」) instead of the cell. Here: the copy made from the sidebar's menu, opened,
a cell typed into; ⌘Z takes the cell back and leaves the copy; ⌘Z again (the table's history spent)
still leaves the copy; back on the sidebar (a click on a row's blank), ⌘Z is the folders' again and
undoes 创建副本. Then a column's menu: the title field's 删除字段/列 is there, dimmed; another
column's asks first (the records it holds data in) and goes; ⌘Z brings it back with its cells.

Then: ⌘Z / ⌘⇧Z while a cell is being edited undo and redo the typing in it, and with none left do
nothing (the table's own steps wait until the cell is left); 创建副本 opens the copy as soon as it is
written; and, as in VS Code's Explorer, a page open and typed into, 创建副本 from a row's right-click
menu leaves the keys on the sidebar, so ⌘Z undoes the copy and the page keeps its words.

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
(source / "阅读清单.md").write_text("三体\n")

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


# 4. ⌘Z in the cell being edited: its typing, back and forth; with none left, nothing (not the table's steps)
ROW_2 = "460 185"
FIELD = f"""row Sources
wait 1500
click {ROW_2}
wait 300
type 球
key enter
wait 600
click {TITLE_1}
wait 300
type 改过
wait 300
"""
lib = run("field-undo", FIELD + "key cmd-z\nwait 300\nmove 1200 800\nshot field-undone\nkey cmd-z\nwait 300\nkey cmd-z\nwait 300\nkey enter\nwait 800\n", source)
assert column(lib, "Sources.sqlite", "名称") == ["三体", "球", "雪崩"], column(lib, "Sources.sqlite", "名称")
lib = run("field-redo", FIELD + "key cmd-z\nwait 300\nkey cmd-shift-z\nwait 300\nkey enter\nwait 800\n", source)
assert column(lib, "Sources.sqlite", "名称") == ["改过", "球", "雪崩"], column(lib, "Sources.sqlite", "名称")
# left, the table's ⌘Z again: the committed 改过 goes, then 球
lib = run("field-left", FIELD + "key enter\nwait 600\nkey cmd-z\nwait 600\nkey cmd-z\nwait 800\n", source)
assert column(lib, "Sources.sqlite", "名称") == ["三体", "球状闪电", "雪崩"], column(lib, "Sources.sqlite", "名称")

# 5. 创建副本 opens the copy once it is written: at once, no 「文档不存在」
lib = run("copy-opens", "rrow Sources\nwait 500\nrow 创建副本\nwait 1500\nmove 1200 800\nshot copy-opened\nshowing Sources 副本\n", source)
assert files(lib) == ["Sources 副本.sqlite", "Sources.sqlite"], files(lib)

# 6. a page typed into, 创建副本 of it from the sidebar's right-click menu: ⌘Z undoes the copy, the page keeps its words
PAGE = """row 阅读清单
wait 1200
click 700 187
key cmd-down
type  黑暗森林
wait 1500
rrow 阅读清单
wait 500
row 创建副本
wait 3000
move 1200 800
shot page-copied
key cmd-z
wait 2500
shot page-copy-undone
"""
lib = run("page-copy", PAGE, source)
pages = sorted(p.name for p in lib.glob("*.md"))
assert pages == ["阅读清单.md"], pages
assert "黑暗森林" in (lib / "阅读清单.md").read_text(), (lib / "阅读清单.md").read_text()

print("table_undo: all passed", work)
