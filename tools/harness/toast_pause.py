"""The toast's clock (Sonner's, as shadcn ships it): shown 4 s, paused while the pointer is over it.

Build with --release --features devtools, then run this file. FDL_BIN selects another build;
TOAST_CASES selects comma-separated cases. Every library and screenshot stays in a temporary
directory; each run quits the app. Requires Pillow.

Cases: life (no pointer on it: still whole about 3 s in, past the old 3 s life, and gone by 5 s),
hover (the pointer on it for 6 s keeps it whole; once the pointer leaves it goes on with what was
left: whole 2 s later, gone 5 s later),
look (light and dark shots of the toast, kept for review).
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image, ImageChops

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-toast-pause-"))
W, H = 1300, 850
# The toast: 356 wide, centred, 32 below the window's top.
BOX = (W // 2 - 178, 32, W // 2 + 178, 84)
# 添加到“置顶” on a page's row menu says so in a toast.
SHOW = "rrow Loose\nwait 300\nrow 添加到“置顶”\nwait 300\n"


def run(name, steps, appearance="light"):
    src = work / (name + "-src")
    (src / ".quiet").mkdir(parents=True)
    (src / ".quiet" / "preferences.json").write_text(json.dumps({"appearance": appearance, "english": False, "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False}))
    (src / "Loose.xml").write_text("<title>Loose</title>\n<p>text</p>\n")
    out = work / name
    (work / (name + ".txt")).write_text(f"resize {W} {H}\nwait 300\nmove 1290 840\nwait 1200\nshot none\n" + steps)
    subprocess.run([sys.executable, str(here / "run.py"), str(work / (name + ".txt")), str(src), str(out)],
                   env=dict(os.environ, FDL_NOBLINK="1", FDL_ANIMATE="1"), check=True, timeout=180)
    return out


def region(f, name):
    im = Image.open(f / (name + ".png")).convert("RGB")
    s = im.width / W
    return im.crop(tuple(round(v * s) for v in BOX))


def shown(f, name):
    return ImageChops.difference(region(f, "none"), region(f, name)).getbbox() is not None


def whole(f, name):
    """Drawn at full opacity: the same as the first shot of it."""
    return ImageChops.difference(region(f, "first"), region(f, name)).getbbox() is None


def case_life():
    # `shot first` comes 300 ms after the toast; a shot itself takes a moment, so the waits leave room.
    f = run("life", SHOW + "shot first\nwait 2600\nshot at-3\nwait 2000\nshot at-5\n")
    assert shown(f, "first"), f"no toast; see {f}"
    assert whole(f, "at-3"), f"the toast is fading or gone 3 s in; see {f}"
    assert not shown(f, "at-5"), f"the toast is still there 5 s in; see {f}"


def case_hover():
    x, y = W // 2, (BOX[1] + BOX[3]) // 2
    steps = SHOW + f"shot first\nmove {x} {y}\nwait 6000\nshot held\nmove 1290 840\nwait 2000\nshot after-2\nwait 3000\nshot after-5\n"
    f = run("hover", steps)
    assert shown(f, "first"), f"no toast; see {f}"
    assert whole(f, "held"), f"the toast faded or went with the pointer on it; see {f}"
    assert whole(f, "after-2"), f"the toast didn't go on with the time it had left; see {f}"
    assert not shown(f, "after-5"), f"the toast is still there 5 s after the pointer left; see {f}"


def case_look():
    for look in ("light", "dark"):
        f = run("look-" + look, SHOW + "shot toast\n", appearance=look)
        print("look:", f / "toast.png")


CASES = {n[5:]: f for n, f in globals().items() if n.startswith("case_")}

if __name__ == "__main__":
    picked = [c for c in os.environ.get("TOAST_CASES", ",".join(CASES)).split(",") if c]
    for name in picked:
        CASES[name]()
        print("ok", name)
    print("all passed;", work)
