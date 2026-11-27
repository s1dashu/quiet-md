"""Toasts as a component (design::toast), in the 组件库's 提示 page, light and dark.

Build with --release --features devtools, then run this file. FDL_BIN selects another build. The
panel opens alone (QUIET_MD_COMPONENTS); every screenshot stays in a temporary directory, printed
at the end. Requires Pillow.

Checks, in each look: three toasts at once stack (the two behind peek out under the front one),
the pointer on the stack spreads them out (taller), Esc closes the front one and the next takes
its place, a toast's 撤销 runs its action (the panel says so at its foot) and closes the toast.
In the app (case reading): keys typed in 阅读模式 bring one 「当前为阅读模式」 toast, not one per
key, and its 开始编辑 switches to editing (a letter typed then lands in the file).
Shots kept: the page's samples (each tone, a description, an action, a close button, a long text
wrapped to three lines), the stack, spread out, after Esc.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image, ImageChops

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-toast-stack-"))
W, H = 1400, 900
PAGE = "click 31 318\nwait 500\n"          # 提示 in the panel's list
LOOK = {"light": "click 1237 26\nwait 300\n", "dark": "click 1283 26\nwait 300\n"}
PARK = f"move {W - 10} {H - 10}\nwait 300\n"
STACK = "click 298 251\n"                  # 连出三条（堆叠）
UNDO = "click 538 208\n"                   # 带撤销（6 秒）
AREA = (W // 2 - 190, 20, W // 2 + 190, 420)  # where the toasts sit


def run(name, steps, lib=None):
    out = work / name
    (work / (name + ".txt")).write_text(f"resize {W} {H}\nwait 1500\n" + steps)
    env = dict(os.environ, FDL_NOBLINK="1", FDL_ANIMATE="1")
    if lib is None:
        lib = work / "lib"
        lib.mkdir(exist_ok=True)
        env["QUIET_MD_COMPONENTS"] = "1"
    subprocess.run([sys.executable, str(here / "run.py"), str(work / (name + ".txt")), str(lib), str(out)], env=env, check=True, timeout=180)
    return out


def img(f, name):
    im = Image.open(f / (name + ".png")).convert("RGB")
    s = im.width / W
    return im, s


def extent(f, name, base):
    """How tall the toasts are: the rows of the area that differ from the shot without them."""
    a, s = img(f, name)
    b, _ = img(f, base)
    box = tuple(round(v * s) for v in AREA)
    d = ImageChops.difference(a.crop(box), b.crop(box)).getbbox()
    return 0 if d is None else (d[3] - d[1]) / s


def case(look):
    steps = PAGE + LOOK[look] + PARK + "shot samples\n" + STACK + PARK + "wait 300\nshot stack\n"
    steps += f"move {W // 2} 60\nwait 600\nshot spread\n" + PARK + "key escape\nwait 600\nshot esc\n"
    f = run(look, steps)
    one, three, spread, after = (extent(f, n, "samples") for n in ("samples", "stack", "spread", "esc"))
    assert one == 0, f"a toast before any was asked for; see {f}"
    assert three > 0, f"no toasts; see {f}"
    assert spread > three + 60, f"the stack didn't spread out under the pointer ({three} → {spread}); see {f}"
    stack, _ = img(f, "stack")
    esc, s = img(f, "esc")
    box = tuple(round(v * s) for v in AREA)
    assert ImageChops.difference(stack.crop(box), esc.crop(box)).getbbox(), f"Esc left the front toast; see {f}"
    # the action: 撤销 runs and the toast goes
    g = run(look + "-undo", PAGE + LOOK[look] + PARK + "shot before\n" + UNDO + PARK + "wait 300\nshot shown\n"
            + f"click {W // 2 + 140} 60\n" + PARK + "wait 800\nshot done\n")
    before, s = img(g, "before")
    done, _ = img(g, "done")
    foot = tuple(round(v * s) for v in (200, H - 80, W, H))
    assert extent(g, "shown", "before") > 0, f"no toast with 撤销; see {g}"
    assert extent(g, "done", "before") == 0, f"the toast stayed after its action; see {g}"
    assert ImageChops.difference(before.crop(foot), done.crop(foot)).getbbox(), f"the action didn't run; see {g}"
    print(look, "shots:", f, g)


def case_reading():
    lib = work / "reading-src"
    (lib / ".quiet").mkdir(parents=True)
    (lib / ".quiet" / "preferences.json").write_text(json.dumps({"appearance": "light", "english": False, "seen_version": "9.9.9", "auto_update": False, "quick_hotkey": False}))
    (lib / "Page.xml").write_text("<title>Page</title>\n<p>one</p>\n<p>two</p>\n")
    # 阅读模式 from the "…" menu; four keys typed; the front toast's 开始编辑 (its right end)
    steps = PARK + "row Page\nwait 800\nshot base\n" + f"click {W - 25} 24\nwait 400\nrow 阅读模式\nwait 400\nkey escape\nwait 300\n"
    steps += "click 700 259\ntype abcd\nwait 300\n" + PARK + "shot typed\n" + f"move {W // 2 + 130} 59\nwait 400\nshot spread\nclick {W // 2 + 130} 59\nwait 600\ntype Z\nwait 1500\n"
    f = run("reading", steps, lib)
    # one toast, not four: spread out, the stack is that card and 已切换为阅读模式 under it
    tall = extent(f, "spread", "base")
    assert 0 < tall < 200, f"the read-only toast piled up ({tall}pt); see {f}"
    text = (f / "lib" / "Page.xml").read_text()
    assert "Z" in text and "abcd" not in text, f"开始编辑 didn't switch to editing: {text!r}; see {f}"
    print("reading shots:", f)


if __name__ == "__main__":
    for look in ("light", "dark"):
        case(look)
        print("ok", look)
    case_reading()
    print("ok reading")
    print("all passed;", work)
