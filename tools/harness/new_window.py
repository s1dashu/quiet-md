"""在新窗口中打开 sizes the window after the main window it came from, and takes its sidebar (scratch library).

From the sidebar row's 「…」 with the main window at 1100 × 760 and its sidebar open: the new
window is 1100 × 760 with its sidebar open, as wide. With the main window's sidebar closed (⌘\\)
and the window at 1000 × 700, the document's own 「…」 › 在新窗口中打开: 1000 × 700 with the
sidebar closed. Closing the sidebar in that new window leaves the main window's as it was, and
the item in a document's own window opens no further window (the one showing it comes forward).

Build with --release --features devtools. FDL_BIN selects another build.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-new-window-"))
ids = ["aaaaaaaa-0000-0000-0000-000000000001", "aaaaaaaa-0000-0000-0000-000000000002"]
source = work / "source"
(source / "docs").mkdir(parents=True)
(source / "index.json").write_text(json.dumps({i: {"created": 1, "modified": 1, "opened": 1 + n} for n, i in enumerate(ids)}))
(source / "preferences.json").write_text(json.dumps({
    "theme": "craft", "appearance": "light", "english": True, "seen_version": "0.1.10",
    "auto_update": False, "quick_hotkey": False, "remote_images": False,
}))
for i, title in zip(ids, ["Alpha", "Beta"]):
    (source / "docs" / (i + ".xml")).write_text(f"<title>{title}</title>\n<p>{title} body</p>\n")

# At 1000 wide the page's 「…」 is at 976,24.
steps = """resize 1100 760
wait 1500
move 1050 700
frame
rrow Alpha
wait 500
row Open in new window
wait 1500
windows 2
window 1
wait 500
showing Alpha
frame
key cmd-\\
wait 400
frame
window 0
wait 400
frame
row Beta
wait 800
showing Beta
key cmd-\\
wait 400
resize 1000 700
wait 600
frame
click 976 24
wait 500
row Open in new window
wait 1500
windows 3
window 2
wait 500
showing Beta
frame
click 976 24
wait 500
row Open in new window
wait 1500
windows 3
quit
"""
script = work / "steps.txt"
script.write_text(steps)
run = work / "run"
print(f"Artifacts: {work}", flush=True)
subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(run)],
               env=dict(os.environ, FDL_NOBLINK="1"), check=True, timeout=120)
log = (run / "log").read_text(errors="replace")
frames = [tuple(m.groups()) for m in re.finditer(r"devtools: frame (\d+) (\d+) sidebar (\w+) (\d+)", log)]
assert len(frames) == 6, f"frames logged: {frames}; see {run / 'log'}"
main, first, first_closed, main_after, main_closed, second = frames
assert main[:3] == ("1100", "760", "true"), f"main window: {main}"
# the sidebar row's 「…」: the main window's size, its sidebar open and as wide
assert first == main, f"new window {first}, main window {main}"
assert first_closed[2] == "false", f"new window's sidebar didn't close: {first_closed}"
# closing it there leaves the main window's sidebar as it was
assert main_after == main, f"main window changed: {main_after}"
assert main_closed[:3] == ("1000", "700", "false"), f"main window: {main_closed}"
# the document's 「…」: the main window's size now, its sidebar closed
assert second[:3] == ("1000", "700", "false"), f"new window {second}"

left = subprocess.run(["pgrep", "-fl", "quiet_md"], capture_output=True, text=True).stdout
assert str(run / "lib") not in left and str(script) not in left, f"still running: {left}"
print(f"ok: {frames}")
