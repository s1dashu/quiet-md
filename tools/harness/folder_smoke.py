"""Run the folder workspace smoke test against a temporary Git repository.

Requires a release binary: cargo build --release
Optional: FDL_BIN=/path/to/quiet_md python3 tools/harness/folder_smoke.py
The temporary fixture and screenshots are retained for inspection.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

repo = Path(__file__).resolve().parents[2]
work = Path(tempfile.mkdtemp(prefix="quiet-folder-smoke-"))
folder = work / "folder"
folder.mkdir()
original = b"# Readme\n\n- [ ] Task\n\n```mermaid\nflowchart LR\nA-->B\n```\n"
(folder / "README.md").write_bytes(original)
(folder / "second.mdx").write_text("# Second\n", encoding="utf-8")
env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull, GIT_TERMINAL_PROMPT="0")
def git(*args):
    subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-C", str(folder), *args], check=True, env=env, stdout=subprocess.DEVNULL)
git("init", "-b", "main")
git("config", "user.name", "Test")
git("config", "user.email", "test@example.invalid")
git("add", ".")
git("commit", "-m", "fixture")
(folder / "second.mdx").write_text("# Staged\n", encoding="utf-8")
git("add", "second.mdx")
(folder / "second.mdx").write_text("# Changed\n\n<Component value={2} />\n", encoding="utf-8")
script = work / "steps.txt"
script.write_text(f"""pick {folder}
key cmd-o
wait 2500
key cmd-alt-2
wait 600
folder-state
key cmd-alt-1
wait 600
folder-state
key cmd-alt-3
wait 800
folder-state
shot initial
click 85 127
wait 800
folder-state
nav back 400 150
wait 800
folder-state
nav forward 400 150
wait 800
folder-state
swipe 400 150 120 0
wait 800
folder-state
swipe 1050 150 120 0
wait 800
folder-state
swipe 1050 150 -120 0
wait 800
folder-state
key alt-left
wait 800
folder-state
key alt-right
wait 800
folder-state
shot second
click 185 193
wait 800
folder-state
shot diff-index
click 229 193
wait 800
folder-state
shot diff-worktree
click 85 127
wait 500
key cmd-alt-1
wait 500
click 450 130
key cmd-end
type  saved
wait 500
folder-state
key cmd-s
wait 800
folder-state
shot saved
key alt-left
wait 800
key cmd-alt-1
wait 500
click 450 130
key cmd-end
type  edited
wait 800
folder-state
key cmd-z
wait 500
folder-state
key cmd-shift-z
wait 500
folder-state
wait 5000
key cmd-s
wait 1000
folder-state
shot conflict
""", encoding="utf-8")
process = subprocess.Popen([sys.executable, str(repo / "tools/harness/run.py"), str(script), str(repo / "tools/harness/lib_clean"), str(work / "run")], env=env)
log_path = work / "run/log"
external = b"# External change\n"
changed = False
unchanged = False
deadline = time.monotonic() + 90
while process.poll() is None and time.monotonic() < deadline:
    count = log_path.read_text(errors="replace").count("devtools: folder-state ") if log_path.exists() else 0
    if count >= 11 and not unchanged:
        unchanged = (folder / "README.md").read_bytes() == original
    if count >= 18:
        (folder / "README.md").write_bytes(external)
        changed = True
        break
    time.sleep(0.1)
try:
    result = process.wait(timeout=30)
except subprocess.TimeoutExpired:
    process.terminate()
    process.wait()
    raise SystemExit(f"Timed out. Fixture and log: {work}")
log = (work / "run/log").read_text(errors="replace")
states = [line.split("devtools: folder-state ", 1)[1] for line in log.splitlines() if "devtools: folder-state " in line]
expected = ["README.md", "README.md", "README.md", "second.mdx", "README.md", "second.mdx", "second.mdx", "README.md", "second.mdx", "README.md", "second.mdx"]
actual = [state.split(" mode=", 1)[0].removeprefix("path=") for state in states]
print(f"Fixture, screenshots and log: {work}")
print(f"Navigation paths: {actual}")
diffs = states[11:13]
saved = states[13:15]
editing = states[15:19]
if (result or actual[:11] != expected or not unchanged or not changed or (folder / "README.md").read_bytes() != external
        or len(states) < 3 or "mode=preview" not in states[0] or "mode=source" not in states[1] or "mode=split" not in states[2] or "branch=main" not in states[0] or len(diffs) != 2 or "+# Staged" not in diffs[0] or "+# Changed" not in diffs[1] or len(saved) != 2 or "dirty=true" not in saved[0] or "dirty=false" not in saved[1]
        or b"saved" not in (folder / "second.mdx").read_bytes() or len(editing) != 4 or "dirty=true" not in editing[0] or "dirty=false" not in editing[1]
        or "dirty=true" not in editing[2] or "dirty=true" not in editing[3] or "File changed" not in editing[3]):
    print("FAIL: inspect log and screenshots", file=sys.stderr)
    sys.exit(1)
print("PASS: folder open, Git fixture, navigation, undo/redo and external-change protection")
