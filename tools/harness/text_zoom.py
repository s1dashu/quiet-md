"""放大 / 缩小 / 实际大小 (⌘+ ⌘− ⌘0): the note's text size, the line being read held.

Build with --release --features devtools, then run this file. FDL_BIN selects
another build; ZOOM_CASES selects comma-separated cases. Every library, script
and screenshot stays in a temporary directory; each run quits the app.

Cases: steps (⌘= twice is 外观 › 正文字号's 大, ⌘− back, ⌘0 the theme's size,
written to the preferences), hold (the caret's line keeps its height through
a zoom), badge (the percentage shows in the note's corner, then goes),
shots (100% / zoomed in / zoomed out, light and dark, kept for review).
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
work = Path(tempfile.mkdtemp(prefix="quiet-text-zoom-"))
doc_id = "aaaaaaaa-0000-0000-0000-000000000001"
W, H = 1200, 850


def run(name, steps, appearance="light", body=None):
    source = work / (name + "-source")
    (source / "docs").mkdir(parents=True)
    (source / "index.json").write_text(json.dumps({doc_id: {"created": 1, "modified": 1, "opened": 1}}))
    (source / "preferences.json").write_text(json.dumps({
        "theme": "craft", "appearance": appearance, "english": True, "seen_version": "9.9.9",
        "auto_update": False, "quick_hotkey": False,
    }))
    xml = body or ('<title>Text size</title>\n' + ''.join(
        f'<p>Paragraph {i:03d}{" MIDDLE_MARK" if i == 40 else ""} with a few words to read.</p>\n' for i in range(80)))
    (source / "docs" / f"{doc_id}.xml").write_text(xml)
    folder = work / name
    script = work / (name + ".txt")
    script.write_text(f"resize {W} {H}\nwait 1500\nmove 1150 820\n" + steps + "\nquit\n")
    subprocess.run([sys.executable, str(here / "run.py"), str(script), str(source), str(folder)],
                   env=dict(os.environ, FDL_NOBLINK="1", FDL_ANIMATE="1"), check=True, timeout=120)
    log = (folder / "log").read_text(errors="replace")
    carets = [float(y) for y in re.findall(r"devtools: caret (-?[\d.]+) of", log)]
    return carets, folder


def doc_size(folder):
    """正文字号 as written: the preferences, or the top-level folder's look (W21)."""
    found = {}
    for p in (folder / "lib").rglob("*.json"):
        if "before-folders" in str(p):
            continue
        try:
            d = json.loads(p.read_text())
        except Exception:
            continue
        if isinstance(d, dict) and "doc_size" in d:
            found[str(p.relative_to(folder))] = d["doc_size"]
    return found


to_middle = "key cmd-f\ntype MIDDLE_MARK\nkey escape\nkey right\nwait 500\nwheel 700 400 -150\nwait 500\n"
settle = "wait 900\n"


def case_steps():
    _, f = run("steps", to_middle + "key cmd-=\nwait 100\nkey cmd-=\n" + settle)
    assert set(doc_size(f).values()) == {"large"}, f"⌘= twice isn't 大: {doc_size(f)}; see {f}"
    _, f = run("steps-back", to_middle + "key cmd-=\nkey cmd-=\nkey cmd--\nkey cmd--\nkey cmd--\n" + settle)
    assert set(doc_size(f).values()) == {"small"}, f"⌘− thrice from 大 isn't 小: {doc_size(f)}; see {f}"
    _, f = run("steps-reset", to_middle + "key cmd-=\nkey cmd-=\nkey cmd-=\nkey cmd-=\nkey cmd-=\n" + settle + "key cmd-0\n" + settle)
    assert set(doc_size(f).values()) == {""}, f"⌘0 isn't the theme's size: {doc_size(f)}; see {f}"


def case_hold():
    steps = to_middle + "caret\nkey cmd-=\nkey cmd-=\nkey cmd-=\n" + settle + "caret\nkey cmd--\nkey cmd--\nkey cmd--\nkey cmd--\nkey cmd--\n" + settle + "caret\n"
    carets, f = run("hold", steps)
    assert len(carets) == 3, f"caret not on the page: {carets}; see {f}"
    for c in carets[1:]:
        assert abs(c - carets[0]) <= 4, f"the caret's line moved through a zoom: {carets}; see {f}"


def case_badge():
    _, f = run("badge", to_middle + "key cmd-=\nwait 300\nshot during\nwait 1500\nshot after\n")
    from PIL import Image, ImageChops
    box = lambda im: im.crop((round(im.width * 0.82), round(im.height * 0.93), im.width, im.height))
    during, after = (Image.open(f / n).convert("RGB") for n in ("during.png", "after.png"))
    assert ImageChops.difference(box(during), box(after)).getbbox(), f"no percentage in the corner; see {f}"


def case_shots():
    body = ('<title>Weekend reading</title>\n'
            '<p>This week I finished <b>Walden</b>. What stayed with me is how plainly Thoreau puts it: a man is rich in proportion to the number of things he can afford to let alone.</p>\n'
            '<h2>Notes</h2>\n<blockquote><p>I went to the woods because I wished to live deliberately.</p></blockquote>\n'
            '<p>MIDDLE_MARK Reading that, I stopped for a while.</p>\n'
            '<table><colgroup><col width="160"/><col width="200"/></colgroup><thead><tr><th><p>Chapter</p></th><th><p>Pages</p></th></tr></thead>'
            '<tbody><tr><td><p>Economy</p></td><td><p>1–80</p></td></tr></tbody></table>\n'
            '<pre lang="python"><code>pages = 30 * 7\nprint(pages)</code></pre>\n'
            '<p>Equation: <latex>E=mc^2</latex></p>\n' + ''.join(f'<p>More notes {i}.</p>\n' for i in range(10)))
    steps = "key cmd-f\ntype MIDDLE_MARK\nkey escape\nkey right\nwait 700\nshot zoom-100\n"
    steps += "key cmd-=\nkey cmd-=\nkey cmd-=\nkey cmd-=\nwait 400\nshot zoom-in\nwait 1500\n"
    steps += "key cmd-0\nkey cmd--\nkey cmd--\nkey cmd--\nwait 400\nshot zoom-out\n"
    for look in ("light", "dark"):
        _, f = run("shots-" + look, steps, appearance=look, body=body)
        print("shots:", f)


CASES = {n[5:]: f for n, f in globals().items() if n.startswith("case_")}

if __name__ == "__main__":
    picked = [c for c in os.environ.get("ZOOM_CASES", ",".join(CASES)).split(",") if c]
    for name in picked:
        CASES[name]()
        print("ok", name)
    print("all passed;", work)
