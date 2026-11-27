"""A submenu stays open while the pointer crosses its menu's rows on the way to it (Radix's pointer grace).

The document's 「…」 opens 页宽设置's submenu on its left. Moving from 页宽设置 down-left to the
submenu crosses 阅读模式; the submenu must still be open when the pointer gets there. Going straight
down from 页宽设置 to 阅读模式 leaves the grace area at once and closes it; stopping on 阅读模式 inside
the area closes it once the grace runs out (`design::menu::GRACE_MS`). A row whose submenu is open
stays lit, so 页宽设置's fill tells whether the submenu is open.

The harness takes about 125ms a move, so the way across is two moves after leaving the row (Radix
gives 300ms). Build with --release --features devtools, then run this file. Needs Pillow. FDL_BIN
selects another build. Only a scratch library is used.
"""
from pathlib import Path
import subprocess
import sys
import tempfile

from PIL import Image

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-submenu-grace-"))
source = work / "source"
(source / "Inbox").mkdir(parents=True)
(source / "Inbox" / "Start.md").write_text("# Start\n\nstart\n")
run_dir = work / "run"
script = work / "script.txt"
# Window points at 1000 wide: the 「…」 at 904,24; its menu hangs right-aligned under it, 页宽设置
# at y 48–80 and 阅读模式 at 80–112 (x 668–916); the submenu opens to the left of x 664.
script.write_text("""resize 1000 760
wait 2000
row Start
wait 600
move 600 700
wait 200
click 904 24
wait 500
move 870 64
move 860 64
wait 400
shot open
move 809 75
move 775 83
move 700 100
move 655 110
wait 500
shot across
move 760 70
move 860 64
wait 400
move 860 84
move 860 96
wait 400
shot down
move 860 64
wait 400
move 809 75
move 775 83
move 720 96
wait 600
shot stopped
""")
assert subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(run_dir)]).returncode == 0, f"the run failed; see {run_dir / 'log'}"


def lit(name, y):
    """Whether the menu row at window y (its left padding, at 2x) has a fill."""
    r, g, b = Image.open(run_dir / f"{name}.png").convert("RGB").getpixel((1350, y * 2))
    return max(r, g, b) < 252


assert lit("open", 64), f"hovering 页宽设置 opens its submenu: {run_dir}/open.png"
assert lit("across", 64), f"the submenu closed on the way across 阅读模式: {run_dir}/across.png"
assert not lit("down", 64) and lit("down", 96), f"straight down to 阅读模式 should close it: {run_dir}/down.png"
assert not lit("stopped", 64) and lit("stopped", 96), f"stopping on 阅读模式 should close it after the grace: {run_dir}/stopped.png"
print(f"submenu grace: kept on the way across, closed going down and after a stop ({run_dir})")
